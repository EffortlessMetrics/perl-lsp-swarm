//! Generation-bound source region index and byte-span queries.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::collector;
use super::kind::SourceRegionKind;
use super::region::{SourceRegion, last_char_start};

/// Result of classifying a byte range against stored regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeClassification {
    /// The entire range lies inside one proven non-code region.
    Proven {
        /// The covering region kind.
        kind: SourceRegionKind,
    },
    /// The range straddles multiple kinds or lies on a boundary mismatch.
    Ambiguous,
    /// Range endpoints are out of bounds or not on char boundaries.
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

    /// Return completed heredoc spans before normalization, retaining empty
    /// body spans so callers can protect their terminator lines.
    #[must_use]
    pub fn completed_heredoc_spans(&self) -> Vec<SourceRegion> {
        collector::scan_heredoc_regions(&self.source)
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

    /// Innermost non-code region kind at `offset`, or [`SourceRegionKind::Code`].
    #[must_use]
    pub fn kind_at_offset(&self, offset: usize) -> SourceRegionKind {
        if !is_valid_char_offset(&self.source, offset) {
            return SourceRegionKind::Code;
        }
        self.regions
            .iter()
            .rev()
            .find(|region| region.contains_offset(offset))
            .map_or(SourceRegionKind::Code, |region| region.kind)
    }

    /// Classify `[start, end)` against stored regions.
    #[must_use]
    pub fn classify_range(&self, start: usize, end: usize) -> RangeClassification {
        if start > end || end > self.source.len() {
            return RangeClassification::OutOfBounds;
        }
        if !is_valid_char_offset(&self.source, start) || !is_valid_char_offset(&self.source, end) {
            return RangeClassification::OutOfBounds;
        }
        if start == end {
            return RangeClassification::Proven { kind: self.kind_at_offset(start) };
        }

        // Probe the start of the last *character* in the range: `end - 1` lands
        // on a UTF-8 continuation byte when the range ends with multibyte text,
        // making `kind_at_offset` fall back to `Code` and downgrading a
        // genuinely proven range to `Ambiguous`.
        let start_kind = self.kind_at_offset(start);
        let last_inclusive = last_char_start(&self.source, end);
        let end_kind = self.kind_at_offset(last_inclusive);
        if start_kind != end_kind {
            return RangeClassification::Ambiguous;
        }
        for region in &self.regions {
            if region.start < end && start < region.end && !region.contains_range(start, end) {
                return RangeClassification::Ambiguous;
            }
        }
        RangeClassification::Proven { kind: start_kind }
    }

    /// Whether `[start, end)` lies entirely inside one of `allowed` kinds.
    #[must_use]
    pub fn range_fully_within(
        &self,
        start: usize,
        end: usize,
        allowed: &[SourceRegionKind],
    ) -> bool {
        match self.classify_range(start, end) {
            RangeClassification::Proven { kind } => allowed.contains(&kind),
            RangeClassification::Ambiguous | RangeClassification::OutOfBounds => false,
        }
    }
}

/// Hash source text the same way `ParsedSnapshot::from_parse_result` does.
#[must_use]
pub fn hash_source_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn is_valid_char_offset(source: &str, offset: usize) -> bool {
    offset <= source.len() && source.is_char_boundary(offset)
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
