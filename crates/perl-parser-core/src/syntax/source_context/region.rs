//! Half-open source region spans.

use super::kind::SourceRegionKind;

/// A half-open `[start, end)` byte span with a lexical classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRegion {
    /// Inclusive start byte offset (UTF-8).
    pub start: usize,
    /// Exclusive end byte offset (UTF-8).
    pub end: usize,
    /// Classification of bytes in `[start, end)`.
    pub kind: SourceRegionKind,
}

/// Byte offset of the last character starting strictly before `end`.
///
/// Returns `0` when `end` is `0`. Anchors spans on UTF-8 char boundaries instead
/// of `end - 1`, which lands on a continuation byte for multibyte text.
pub(super) fn last_char_start(source: &str, end: usize) -> usize {
    let mut offset = end.min(source.len()).saturating_sub(1);
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

impl SourceRegion {
    /// Construct a region after validating ordering (`start <= end`).
    ///
    /// This does **not** validate UTF-8 char boundaries: a [`SourceRegion`] is a
    /// plain byte span and does not carry the source it indexes. Callers must
    /// supply boundary-aligned offsets — see [`last_char_start`] for anchoring a
    /// span to the character preceding an offset.
    #[must_use]
    pub fn new(start: usize, end: usize, kind: SourceRegionKind) -> Option<Self> {
        if start > end {
            return None;
        }
        Some(Self { start, end, kind })
    }

    /// Whether `offset` lies inside this half-open span.
    #[must_use]
    pub fn contains_offset(self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Whether this region fully covers `[start, end)`.
    #[must_use]
    pub fn contains_range(self, start: usize, end: usize) -> bool {
        self.start <= start && end <= self.end
    }
}
