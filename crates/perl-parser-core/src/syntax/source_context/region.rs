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

impl SourceRegion {
    /// Construct a region after validating char boundaries and ordering.
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
