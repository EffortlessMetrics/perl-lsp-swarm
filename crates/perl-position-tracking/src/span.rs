//! Byte-based span types for source location tracking.
//!
//! This module provides foundational span types used throughout the Perl LSP
//! ecosystem for tracking source locations. These types use byte offsets,
//! which are efficient for the parser but must be converted to line/character
//! positions for LSP communication.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

/// Error returned when a byte span cannot represent a valid half-open range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidByteSpan {
    /// The rejected starting offset.
    pub start: usize,
    /// The rejected ending offset.
    pub end: usize,
}

impl fmt::Display for InvalidByteSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte span start {} exceeds end {}", self.start, self.end)
    }
}

impl std::error::Error for InvalidByteSpan {}

/// A byte-based span representing a range in source text.
///
/// `ByteSpan` uses byte offsets (not character or line positions) for precise
/// and efficient source location tracking. For LSP communication, use
/// [`WireRange`](crate::WireRange) or convert via [`LineStartsCache`](crate::LineStartsCache).
///
/// # Invariants
///
/// - `start <= end` holds for every constructible value: fields are private,
///   [`ByteSpan::new`] orders its arguments, serialization validates the
///   ordering, and fallible construction is available through
///   [`ByteSpan::try_new`];
/// - Both `start` and `end` are valid byte offsets in the source text;
/// - Spans are half-open intervals: `[start, end)`. A zero-length span
///   (`start == end`) anchors at `start` and contains no offsets
///   ([`ByteSpan::contains`](Self::contains) is always false for it).
///
/// # Constructor discipline
///
/// [`ByteSpan::new`], [`ByteSpan::empty`], [`ByteSpan::whole`], and the
/// `From` conversions always produce ordered spans (`new` swaps reversed
/// arguments). Callers that must detect reversed input use
/// [`ByteSpan::try_new`]; deserialization rejects it outright.
///
/// # Example
///
/// ```
/// use perl_position_tracking::ByteSpan;
///
/// let span = ByteSpan::new(0, 10);
/// assert_eq!(span.len(), 10);
/// assert!(!span.is_empty());
///
/// // Extract the spanned text
/// let source = "hello world";
/// let text = span.slice(source);
/// assert_eq!(text, "hello worl");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ByteSpan {
    /// Starting byte offset in the source text (inclusive).
    start: usize,
    /// Ending byte offset in the source text (exclusive).
    end: usize,
}

impl ByteSpan {
    /// Creates a new `ByteSpan` covering the offsets between `start` and
    /// `end`, in either order.
    ///
    /// This constructor is total and ordering-correcting: the returned span
    /// always satisfies `start <= end`, with the arguments swapped when they
    /// arrive reversed. Callers that must reject reversed input instead of
    /// normalizing it use [`ByteSpan::try_new`]; deserialization is strict.
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        if start <= end { Self { start, end } } else { Self { start: end, end: start } }
    }

    /// Creates a `ByteSpan`, failing if the offsets are reversed.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidByteSpan`] when `start > end`.
    #[inline]
    pub const fn try_new(start: usize, end: usize) -> Result<Self, InvalidByteSpan> {
        if start <= end { Ok(Self { start, end }) } else { Err(InvalidByteSpan { start, end }) }
    }

    /// Creates an empty span at the given position.
    #[inline]
    pub const fn empty(pos: usize) -> Self {
        Self { start: pos, end: pos }
    }

    /// Creates a span covering the entire source text.
    #[inline]
    pub fn whole(source: &str) -> Self {
        Self { start: 0, end: source.len() }
    }

    /// Returns the starting byte offset (inclusive).
    #[inline]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Returns the ending byte offset (exclusive).
    #[inline]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Returns the length of this span in bytes.
    #[inline]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if this span is empty (start == end).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns true if this span contains the given byte offset.
    #[inline]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Returns true if this span contains the given span entirely.
    #[inline]
    pub const fn contains_span(&self, other: ByteSpan) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Returns true if this span overlaps with the given span.
    #[inline]
    pub const fn overlaps(&self, other: ByteSpan) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Returns the intersection of two spans, or None if they don't overlap.
    pub fn intersection(&self, other: ByteSpan) -> Option<ByteSpan> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        if start < end { Some(ByteSpan { start, end }) } else { None }
    }

    /// Returns a new span that covers both this span and the given span.
    #[inline]
    pub fn union(&self, other: ByteSpan) -> ByteSpan {
        ByteSpan { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    /// Extracts the slice of source text covered by this span.
    ///
    /// # Panics
    ///
    /// Panics if the span is out of bounds for the source text.
    #[inline]
    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }

    /// Safely extracts the slice of source text, returning None if out of bounds.
    #[inline]
    pub fn try_slice<'a>(&self, source: &'a str) -> Option<&'a str> {
        source.get(self.start..self.end)
    }

    /// Converts to a standard Range.
    #[inline]
    pub const fn to_range(&self) -> Range<usize> {
        self.start..self.end
    }
}

impl fmt::Display for ByteSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl From<Range<usize>> for ByteSpan {
    #[inline]
    fn from(range: Range<usize>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<ByteSpan> for Range<usize> {
    #[inline]
    fn from(span: ByteSpan) -> Self {
        span.start..span.end
    }
}

impl From<(usize, usize)> for ByteSpan {
    #[inline]
    fn from((start, end): (usize, usize)) -> Self {
        Self::new(start, end)
    }
}

impl From<ByteSpan> for (usize, usize) {
    #[inline]
    fn from(span: ByteSpan) -> Self {
        (span.start, span.end)
    }
}

impl Serialize for ByteSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Repr {
            start: usize,
            end: usize,
        }
        Repr { start: self.start, end: self.end }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ByteSpan {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr {
            start: usize,
            end: usize,
        }
        let repr = Repr::deserialize(deserializer)?;
        ByteSpan::try_new(repr.start, repr.end).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

/// Type alias for backward compatibility with `SourceLocation`.
///
/// New code should use [`ByteSpan`] directly.
pub type SourceLocation = ByteSpan;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_span_basics() {
        let span = ByteSpan::new(5, 10);
        assert_eq!(span.start(), 5);
        assert_eq!(span.end(), 10);
        assert_eq!(span.len(), 5);
        assert!(!span.is_empty());
    }

    #[test]
    fn test_empty_span() {
        let span = ByteSpan::empty(5);
        assert_eq!(span.start(), 5);
        assert_eq!(span.end(), 5);
        assert_eq!(span.len(), 0);
        assert!(span.is_empty());
    }

    #[test]
    fn test_new_normalizes_reversed_offsets() {
        // #8740: the trusted constructor is order-correcting, so reversed
        // offsets can never produce a reversed span.
        assert_eq!(ByteSpan::new(7, 3), ByteSpan::new(3, 7));
        assert_eq!(ByteSpan::try_new(3, 7), Ok(ByteSpan::new(3, 7)));
        assert_eq!(ByteSpan::new(7, 3).len(), 4);
        assert_eq!(ByteSpan::new(7, 3).start(), 3);
        assert_eq!(ByteSpan::new(7, 3).end(), 7);
    }

    #[test]
    fn test_try_new_accepts_ordered_offsets() {
        assert_eq!(ByteSpan::try_new(3, 9), Ok(ByteSpan::new(3, 9)));
        assert_eq!(ByteSpan::try_new(5, 5), Ok(ByteSpan::empty(5)));
    }

    #[test]
    fn test_try_new_rejects_reversed_offsets() {
        assert_eq!(ByteSpan::try_new(7, 3), Err(InvalidByteSpan { start: 7, end: 3 }));
    }

    #[test]
    fn test_serde_round_trip_preserves_span() -> Result<(), serde_json::Error> {
        let span = ByteSpan::new(4, 11);
        let json = serde_json::to_string(&span)?;
        assert_eq!(json, r#"{"start":4,"end":11}"#);
        let back: ByteSpan = serde_json::from_str(&json)?;
        assert_eq!(back, span);
        Ok(())
    }

    #[test]
    fn test_serde_rejects_reversed_span() {
        let result: Result<ByteSpan, _> = serde_json::from_str(r#"{"start":9,"end":2}"#);
        assert!(result.is_err(), "deserialization must fail closed on reversed spans");
    }

    #[test]
    fn test_serde_rejects_missing_fields() {
        assert!(serde_json::from_str::<ByteSpan>(r#"{"start":1}"#).is_err());
        assert!(serde_json::from_str::<ByteSpan>(r#"{}"#).is_err());
    }

    #[test]
    fn test_zero_length_anchor_contains_nothing() {
        let anchor = ByteSpan::empty(7);
        assert!(!anchor.contains(7), "zero-length span must not contain its anchor");
        assert!(!anchor.contains(8));
    }

    #[test]
    fn test_contains() {
        let span = ByteSpan::new(5, 10);
        assert!(!span.contains(4));
        assert!(span.contains(5));
        assert!(span.contains(9));
        assert!(!span.contains(10)); // end is exclusive
    }

    #[test]
    fn test_contains_span() {
        let outer = ByteSpan::new(0, 20);
        let inner = ByteSpan::new(5, 15);
        let partial = ByteSpan::new(15, 25);

        assert!(outer.contains_span(inner));
        assert!(!inner.contains_span(outer));
        assert!(!outer.contains_span(partial));
    }

    #[test]
    fn test_overlaps() {
        let a = ByteSpan::new(0, 10);
        let b = ByteSpan::new(5, 15);
        let c = ByteSpan::new(10, 20);
        let d = ByteSpan::new(15, 25);

        assert!(a.overlaps(b)); // partial overlap
        assert!(!a.overlaps(c)); // adjacent (no overlap)
        assert!(!a.overlaps(d)); // disjoint
    }

    #[test]
    fn test_intersection() {
        let a = ByteSpan::new(0, 10);
        let b = ByteSpan::new(5, 15);

        assert_eq!(a.intersection(b), Some(ByteSpan::new(5, 10)));
        assert_eq!(a.intersection(ByteSpan::new(10, 20)), None);
    }

    #[test]
    fn test_union() {
        let a = ByteSpan::new(0, 10);
        let b = ByteSpan::new(5, 15);

        assert_eq!(a.union(b), ByteSpan::new(0, 15));
    }

    #[test]
    fn test_slice() {
        let source = "hello world";
        let span = ByteSpan::new(0, 5);
        assert_eq!(span.slice(source), "hello");
    }

    #[test]
    fn test_try_slice_out_of_bounds_returns_none() {
        let source = "hello";
        assert_eq!(ByteSpan::new(0, 12).try_slice(source), None);
        assert_eq!(ByteSpan::new(0, 5).try_slice(source), Some("hello"));
    }

    #[test]
    fn test_conversions() {
        let span = ByteSpan::new(5, 10);

        // To/from Range
        let range: Range<usize> = span.into();
        assert_eq!(range, 5..10);
        let span2: ByteSpan = (5..10).into();
        assert_eq!(span, span2);

        // To/from tuple
        let tuple: (usize, usize) = span.into();
        assert_eq!(tuple, (5, 10));
        let span3: ByteSpan = (5, 10).into();
        assert_eq!(span, span3);
    }

    #[test]
    fn test_display() {
        let span = ByteSpan::new(5, 10);
        assert_eq!(format!("{}", span), "5..10");
    }

    // --- Additional intersection / union edge cases ---

    #[test]
    fn test_intersection_non_overlapping_returns_none() {
        let a = ByteSpan::new(0, 5);
        let b = ByteSpan::new(10, 20);
        assert_eq!(a.intersection(b), None, "disjoint spans must have no intersection");
        assert_eq!(b.intersection(a), None, "disjoint spans must have no intersection (reversed)");
    }

    #[test]
    fn test_intersection_identical_returns_same() {
        let a = ByteSpan::new(3, 9);
        assert_eq!(a.intersection(a), Some(a), "identical spans intersect as themselves");
    }

    #[test]
    fn test_union_identical_returns_same() {
        let a = ByteSpan::new(3, 9);
        assert_eq!(a.union(a), a, "union of identical span is itself");
    }

    #[test]
    fn test_intersection_nested_equals_inner() {
        let outer = ByteSpan::new(0, 20);
        let inner = ByteSpan::new(5, 15);
        assert_eq!(
            outer.intersection(inner),
            Some(inner),
            "intersection of outer with inner must equal inner"
        );
        assert_eq!(inner.intersection(outer), Some(inner), "intersection is commutative");
    }

    #[test]
    fn test_union_nested_equals_outer() {
        let outer = ByteSpan::new(0, 20);
        let inner = ByteSpan::new(5, 15);
        assert_eq!(outer.union(inner), outer, "union of nested spans must equal outer");
        assert_eq!(inner.union(outer), outer, "union is commutative");
    }

    #[test]
    fn test_intersection_adjacent_returns_none() {
        // Adjacent spans share a boundary point but do not overlap —
        // intersection requires start < end, so adjacent returns None.
        let a = ByteSpan::new(0, 5);
        let b = ByteSpan::new(5, 10);
        assert_eq!(a.intersection(b), None, "adjacent spans have no overlap: intersection is None");
    }

    #[test]
    fn test_union_adjacent_spans_is_minimal_cover() {
        let a = ByteSpan::new(0, 5);
        let b = ByteSpan::new(5, 10);
        assert_eq!(
            a.union(b),
            ByteSpan::new(0, 10),
            "union of adjacent spans covers both without gap"
        );
    }

    #[test]
    fn test_intersection_empty_span_returns_none() {
        // An empty span (start == end) has no interior, so it cannot overlap
        // with any span in a meaningful way.  The implementation requires
        // start < end for Some, so all these must be None.
        let empty = ByteSpan::empty(5);
        let full = ByteSpan::new(3, 10);

        // empty vs non-empty enclosing span
        assert_eq!(
            empty.intersection(full),
            None,
            "zero-length span has no bytes, intersection must be None"
        );
        assert_eq!(
            full.intersection(empty),
            None,
            "zero-length span has no bytes, intersection must be None (reversed)"
        );

        // empty vs empty
        assert_eq!(
            empty.intersection(empty),
            None,
            "two zero-length spans at the same point have no intersection"
        );
    }

    #[test]
    fn test_union_empty_span_equals_non_empty() {
        let empty = ByteSpan::empty(5);
        let full = ByteSpan::new(3, 10);
        // union of a zero-length span with a real span should just be the real span
        assert_eq!(
            empty.union(full),
            ByteSpan::new(3, 10),
            "union with empty span at interior point covers at least the full span"
        );
    }

    #[test]
    fn test_union_non_overlapping_is_minimal_cover() {
        // Two disjoint spans — union must span from the min start to the max end,
        // covering the gap between them.
        let a = ByteSpan::new(0, 5);
        let b = ByteSpan::new(10, 20);
        assert_eq!(
            a.union(b),
            ByteSpan::new(0, 20),
            "union of non-overlapping spans spans the full range"
        );
        assert_eq!(b.union(a), ByteSpan::new(0, 20), "union is commutative");
    }
}
