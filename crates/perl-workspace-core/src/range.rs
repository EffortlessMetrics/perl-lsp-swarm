//! Byte/UTF-8 source ranges.
//!
//! Core facts carry byte offsets into UTF-8 source, never editor-flavoured
//! (UTF-16) positions. UTF-16 conversion is an LSP-boundary concern and must
//! not leak into this substrate. A [`SourceRange`] is a half-open `[start, end)`
//! interval of byte offsets, matching the `span_start_byte`/`span_end_byte`
//! convention used by [`perl_semantic_facts`].

use serde::{Deserialize, Serialize};

/// A half-open `[start_byte, end_byte)` range of byte offsets into UTF-8 source.
///
/// The range is always well-formed: `start_byte <= end_byte` is an invariant
/// established at construction. Offsets are byte offsets into the UTF-8 source
/// text — not character counts, and not UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRange {
    start_byte: u32,
    end_byte: u32,
}

impl SourceRange {
    /// Construct a range, normalising a reversed pair so `start <= end` always
    /// holds. Prefer [`SourceRange::try_new`] when a reversed pair should be
    /// treated as an error rather than silently normalised.
    #[must_use]
    pub fn new(start_byte: u32, end_byte: u32) -> Self {
        if start_byte <= end_byte {
            Self { start_byte, end_byte }
        } else {
            Self { start_byte: end_byte, end_byte: start_byte }
        }
    }

    /// Construct a range, returning `None` if `start_byte > end_byte`.
    ///
    /// Use this in fact producers so a reversed span surfaces as a bug instead
    /// of being silently swapped.
    #[must_use]
    pub fn try_new(start_byte: u32, end_byte: u32) -> Option<Self> {
        if start_byte <= end_byte { Some(Self { start_byte, end_byte }) } else { None }
    }

    /// The inclusive start byte offset.
    #[must_use]
    pub fn start_byte(&self) -> u32 {
        self.start_byte
    }

    /// The exclusive end byte offset.
    #[must_use]
    pub fn end_byte(&self) -> u32 {
        self.end_byte
    }

    /// The length of the range in bytes.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.end_byte - self.start_byte
    }

    /// Whether the range covers zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start_byte == self.end_byte
    }

    /// Whether `byte` falls within `[start_byte, end_byte)`.
    #[must_use]
    pub fn contains(&self, byte: u32) -> bool {
        self.start_byte <= byte && byte < self.end_byte
    }

    /// Whether this range fully contains `other`.
    #[must_use]
    pub fn contains_range(&self, other: &SourceRange) -> bool {
        self.start_byte <= other.start_byte && other.end_byte <= self.end_byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalises_reversed_pair() {
        let r = SourceRange::new(10, 4);
        assert_eq!(r.start_byte(), 4);
        assert_eq!(r.end_byte(), 10);
    }

    #[test]
    fn try_new_rejects_reversed_pair() {
        assert!(SourceRange::try_new(10, 4).is_none());
        assert!(SourceRange::try_new(4, 10).is_some());
        assert!(SourceRange::try_new(7, 7).is_some());
    }

    #[test]
    fn len_and_contains() {
        let r = SourceRange::new(4, 10);
        assert_eq!(r.len(), 6);
        assert!(!r.is_empty());
        assert!(r.contains(4));
        assert!(r.contains(9));
        assert!(!r.contains(10));
        assert!(!r.contains(3));
    }

    #[test]
    fn empty_range() {
        let r = SourceRange::new(5, 5);
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(!r.contains(5));
    }

    #[test]
    fn contains_range_nesting() {
        let outer = SourceRange::new(0, 100);
        let inner = SourceRange::new(10, 20);
        assert!(outer.contains_range(&inner));
        assert!(!inner.contains_range(&outer));
        assert!(outer.contains_range(&outer));
    }
}
