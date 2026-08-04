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

/// Arm the property oracle itself.
///
/// `region_invariants` is the only discriminator the two properties below have.
/// A helper that returned `true` unconditionally would make both properties
/// vacuous and every mutation of the collector invisible, so each rejection
/// branch is exercised directly against hand-built counterexamples.
#[test]
fn region_invariants_rejects_each_violation_it_claims_to_catch() {
    let source = "héllo world";
    let e_end = 'é'.len_utf8() + 1;

    assert!(
        region_invariants(
            source,
            &[
                SourceRegion { start: 0, end: 1, kind: SourceRegionKind::LineComment },
                SourceRegion { start: 1, end: e_end, kind: SourceRegionKind::Pod },
            ],
        ),
        "sorted, disjoint, in-bounds, boundary-aligned regions must be accepted"
    );

    assert!(
        !region_invariants(
            source,
            &[
                SourceRegion { start: 0, end: 6, kind: SourceRegionKind::LineComment },
                SourceRegion { start: 3, end: 8, kind: SourceRegionKind::Pod },
            ],
        ),
        "overlapping regions must be rejected"
    );

    assert!(
        !region_invariants(
            source,
            &[SourceRegion { start: 4, end: 4, kind: SourceRegionKind::Pod }],
        ),
        "an empty region must be rejected"
    );

    assert!(
        !region_invariants(
            source,
            &[SourceRegion { start: 0, end: source.len() + 1, kind: SourceRegionKind::Pod }],
        ),
        "a region past end-of-source must be rejected"
    );

    assert!(
        !region_invariants(
            source,
            &[SourceRegion { start: 2, end: 6, kind: SourceRegionKind::Pod }],
        ),
        "a start inside the 'é' codepoint must be rejected"
    );

    assert!(
        !region_invariants(
            source,
            &[SourceRegion { start: 0, end: 2, kind: SourceRegionKind::Pod }],
        ),
        "an end inside the 'é' codepoint must be rejected"
    );
}

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
