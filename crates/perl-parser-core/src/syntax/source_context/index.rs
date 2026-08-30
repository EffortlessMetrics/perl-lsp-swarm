//! Generation-bound source region index and byte-span queries.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::collector;
use super::kind::SourceRegionKind;
use super::region::{SourceRegion, last_char_start};

/// Result of classifying one source byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetClassification {
    /// The offset names a valid source byte with one compatibility region kind.
    Proven {
        /// The region kind at the source byte.
        kind: SourceRegionKind,
    },
    /// The offset lies inside a UTF-8 scalar rather than at a character boundary.
    InvalidUtf8Boundary,
    /// The offset does not name a source byte.
    ///
    /// An offset equal to `source.len()` is a valid position boundary, but it is
    /// not a byte. Use [`SourceRegionIndex::classify_range_checked`] with an
    /// empty range to classify that boundary.
    OutOfBounds,
}

impl OffsetClassification {
    /// Return the classified kind only when the offset names a valid source byte.
    #[must_use]
    pub const fn proven_kind(self) -> Option<SourceRegionKind> {
        match self {
            Self::Proven { kind } => Some(kind),
            Self::InvalidUtf8Boundary | Self::OutOfBounds => None,
        }
    }
}

/// Historical result of classifying a byte range against stored regions.
///
/// Use [`SourceRegionIndex::classify_range_checked`] when invalid UTF-8
/// boundaries and empty source positions must remain explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeClassification {
    /// The entire range resolves to one compatibility region kind.
    Proven {
        /// The covering region kind.
        kind: SourceRegionKind,
    },
    /// The range straddles multiple kinds or lies on a boundary mismatch.
    Ambiguous,
    /// Range endpoints are out of bounds or not on char boundaries.
    OutOfBounds,
}

/// Checked result of classifying a source byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRangeClassification {
    /// The entire non-empty range resolves to one compatibility region kind.
    ///
    /// `Code` is still complement-derived in the current index. It becomes
    /// positive code evidence only after the complete-partition cutover tracked
    /// by #13991.
    Proven {
        /// The covering region kind.
        kind: SourceRegionKind,
    },
    /// A valid empty range describes a boundary between source bytes.
    EmptyBoundary {
        /// Kind of the character immediately before the boundary, if any.
        left: Option<SourceRegionKind>,
        /// Kind of the character beginning at the boundary, if any.
        right: Option<SourceRegionKind>,
    },
    /// The range straddles multiple kinds or lies on a boundary mismatch.
    Ambiguous,
    /// At least one endpoint lies inside a UTF-8 scalar.
    InvalidUtf8Boundary,
    /// Range endpoints are reversed or outside the source.
    OutOfBounds,
}

/// Immutable index of non-code source regions for one snapshot.
#[derive(Debug, Clone)]
pub struct SourceRegionIndex {
    content_hash: u64,
    source: Arc<str>,
    regions: Vec<SourceRegion>,
}

impl SourceRegionIndex {
    /// Build an index for `source`, hashing content with the workspace-default scheme.
    #[must_use]
    pub fn build(source: &str) -> Self {
        Self::build_with_hash(source, hash_source_content(source))
    }

    /// Build an index for `source` with a caller-supplied content hash.
    #[must_use]
    pub fn build_with_hash(source: &str, content_hash: u64) -> Self {
        let regions = collector::collect_regions(source);
        Self::from_regions(source, content_hash, regions)
    }

    /// Like [`build_with_hash`](Self::build_with_hash) but clones the caller's
    /// `Arc<str>` instead of allocating a new one from `&str`. Use this when the
    /// caller already owns the source text as `Arc<str>` to avoid a wasteful
    /// allocate + memcpy round-trip (#5526).
    #[must_use]
    pub fn build_with_hash_from_arc(source: Arc<str>, content_hash: u64) -> Self {
        let regions = collector::collect_regions(&source);
        let source_len = source.len();
        let regions = normalize_regions(regions, source_len);
        Self { content_hash, source, regions }
    }

    /// Construct from pre-normalized regions (tests and `with_overrides`).
    #[must_use]
    pub fn from_regions(source: &str, content_hash: u64, regions: Vec<SourceRegion>) -> Self {
        let source_len = source.len();
        let regions = normalize_regions(regions, source_len);
        Self { content_hash, source: Arc::from(source), regions }
    }

    /// Content hash bound to this index.
    #[must_use]
    pub const fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// Number of stored non-code regions.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Borrow the sorted non-overlapping region list.
    #[must_use]
    pub fn regions(&self) -> &[SourceRegion] {
        &self.regions
    }

    /// Return a copy with additional override regions merged in.
    ///
    /// PR1 identity stub for future semantic-island overlays: overrides are merged
    /// and normalized like build-time regions.
    #[must_use]
    pub fn with_overrides(&self, overrides: &[SourceRegion]) -> Self {
        let mut regions = self.regions.clone();
        regions.extend_from_slice(overrides);
        Self {
            content_hash: self.content_hash,
            source: Arc::clone(&self.source),
            regions: normalize_regions(regions, self.source.len()),
        }
    }

    /// Classify one valid source byte without converting invalid input to `Code`.
    #[must_use]
    pub fn classify_offset(&self, offset: usize) -> OffsetClassification {
        if offset >= self.source.len() {
            return OffsetClassification::OutOfBounds;
        }
        if !self.source.is_char_boundary(offset) {
            return OffsetClassification::InvalidUtf8Boundary;
        }
        OffsetClassification::Proven { kind: self.kind_at_valid_offset(offset) }
    }

    /// Compatibility-only region-kind view for one offset.
    ///
    /// Non-`Proven` offsets (invalid UTF-8 interior bytes, EOF, or past-EOF
    /// offsets) return [`SourceRegionKind::Code`] to preserve the historical
    /// API. This fallback is not proof that the byte is code. New precision- or
    /// edit-authorizing callers must use
    /// [`classify_offset`](Self::classify_offset).
    #[must_use]
    pub fn kind_at_offset(&self, offset: usize) -> SourceRegionKind {
        self.classify_offset(offset).proven_kind().unwrap_or(SourceRegionKind::Code)
    }

    /// Classify `[start, end)` through the historical compatibility result.
    ///
    /// Invalid UTF-8 boundaries collapse into [`RangeClassification::OutOfBounds`],
    /// and empty ranges retain the prior right-side/`Code` fallback. Use
    /// [`classify_range_checked`](Self::classify_range_checked) for proof.
    #[must_use]
    pub fn classify_range(&self, start: usize, end: usize) -> RangeClassification {
        match self.classify_range_checked(start, end) {
            SourceRangeClassification::Proven { kind } => RangeClassification::Proven { kind },
            SourceRangeClassification::EmptyBoundary { right, .. } => {
                RangeClassification::Proven { kind: right.unwrap_or(SourceRegionKind::Code) }
            }
            SourceRangeClassification::Ambiguous => RangeClassification::Ambiguous,
            SourceRangeClassification::InvalidUtf8Boundary
            | SourceRangeClassification::OutOfBounds => RangeClassification::OutOfBounds,
        }
    }

    /// Classify `[start, end)` without hiding empty or invalid-boundary input.
    #[must_use]
    pub fn classify_range_checked(&self, start: usize, end: usize) -> SourceRangeClassification {
        if start > end || end > self.source.len() {
            return SourceRangeClassification::OutOfBounds;
        }
        if !self.source.is_char_boundary(start) || !self.source.is_char_boundary(end) {
            return SourceRangeClassification::InvalidUtf8Boundary;
        }
        if start == end {
            let left = if start == 0 {
                None
            } else {
                Some(self.kind_at_valid_offset(last_char_start(&self.source, start)))
            };
            let right = if start == self.source.len() {
                None
            } else {
                Some(self.kind_at_valid_offset(start))
            };
            return SourceRangeClassification::EmptyBoundary { left, right };
        }

        // Probe the start of the last *character* in the range: `end - 1` lands
        // on a UTF-8 continuation byte when the range ends with multibyte text,
        // making a byte classifier reject the offset and downgrading a
        // genuinely uniform range to `Ambiguous`.
        let start_kind = self.kind_at_valid_offset(start);
        let last_inclusive = last_char_start(&self.source, end);
        let end_kind = self.kind_at_valid_offset(last_inclusive);
        if start_kind != end_kind {
            return SourceRangeClassification::Ambiguous;
        }
        for region in &self.regions {
            if region.start < end && start < region.end && !region.contains_range(start, end) {
                return SourceRangeClassification::Ambiguous;
            }
        }
        SourceRangeClassification::Proven { kind: start_kind }
    }

    /// Whether a non-empty `[start, end)` range lies entirely inside one of `allowed` kinds.
    #[must_use]
    pub fn range_fully_within(
        &self,
        start: usize,
        end: usize,
        allowed: &[SourceRegionKind],
    ) -> bool {
        match self.classify_range_checked(start, end) {
            SourceRangeClassification::Proven { kind } => allowed.contains(&kind),
            SourceRangeClassification::EmptyBoundary { .. }
            | SourceRangeClassification::Ambiguous
            | SourceRangeClassification::InvalidUtf8Boundary
            | SourceRangeClassification::OutOfBounds => false,
        }
    }

    fn kind_at_valid_offset(&self, offset: usize) -> SourceRegionKind {
        self.regions
            .iter()
            .rev()
            .find(|region| region.contains_offset(offset))
            .map_or(SourceRegionKind::Code, |region| region.kind)
    }
}

/// Hash source text the same way `ParsedSnapshot::from_parse_result` does.
#[must_use]
pub fn hash_source_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Enforce the stored-region invariant: sorted, non-overlapping, in bounds, and
/// never `Code`.
///
/// Overlap resolution is delegated to [`collector::coalesce_regions`] so that
/// caller-supplied overrides obey exactly the same precedence-and-split rule as
/// build-time regions. `Code` is dropped *before* the sweep: it holds the top
/// precedence slot and would otherwise mask every real region it overlaps.
fn normalize_regions(mut regions: Vec<SourceRegion>, source_len: usize) -> Vec<SourceRegion> {
    regions.retain(|region| {
        region.kind != SourceRegionKind::Code
            && region.start < region.end
            && region.end <= source_len
    });
    collector::coalesce_regions(regions, source_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_merges_adjacent_same_kind() {
        let regions = vec![
            SourceRegion { start: 0, end: 2, kind: SourceRegionKind::Pod },
            SourceRegion { start: 2, end: 5, kind: SourceRegionKind::Pod },
        ];
        let index = SourceRegionIndex::from_regions("xxxxx", 0, regions);
        assert_eq!(index.region_count(), 1);
        assert_eq!(index.regions()[0].end, 5);
    }
}
