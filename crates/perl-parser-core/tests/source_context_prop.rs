use perl_parser_core::{RangeClassification, SourceRegion, SourceRegionIndex, SourceRegionKind};
use proptest::prelude::*;

fn region_invariants(source: &str, regions: &[SourceRegion]) -> bool {
    let len = source.len();
    for (left, right) in regions.iter().zip(regions.iter().skip(1)) {
        if left.end > right.start {
            return false;
        }
    }
    regions.iter().all(|region| {
        region.start < region.end
            && region.end <= len
            && source.is_char_boundary(region.start)
            && source.is_char_boundary(region.end)
    })
}

// The alphabet deliberately includes multibyte characters (`é`, `€`, `😀`) so the
// char-boundary half of `region_invariants` is actually armed: a pure-ASCII
// alphabet cannot falsify a mid-codepoint offset. The bound is a single
// repetition — `[...]*{0,200}` is nested repetition, so the bound was inert.
const SOURCE_ALPHABET: &str = r#"[a-zA-Z0-9_ \t\n\r#"'`/\\{}\[\]();,=é€😀]{0,200}"#;

proptest! {
    #[test]
    fn random_sources_maintain_region_invariants(body in SOURCE_ALPHABET) {
        let index = SourceRegionIndex::build(&body);
        prop_assert!(region_invariants(&body, index.regions()));
        for region in index.regions() {
            prop_assert_ne!(region.kind, SourceRegionKind::Code);
        }
    }

    /// Every offset a region claims must classify back to that region's kind, and
    /// the region's own byte range must classify as proven.
    #[test]
    fn region_offsets_round_trip_through_classification(body in SOURCE_ALPHABET) {
        let index = SourceRegionIndex::build(&body);
        for region in index.regions() {
            prop_assert_eq!(index.kind_at_offset(region.start), region.kind);
            prop_assert_eq!(
                index.classify_range(region.start, region.end),
                RangeClassification::Proven { kind: region.kind }
            );
        }
    }
}
