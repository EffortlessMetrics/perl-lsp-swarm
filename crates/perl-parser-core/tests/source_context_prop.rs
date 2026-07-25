use perl_parser_core::{SourceRegion, SourceRegionIndex, SourceRegionKind};
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

proptest! {
    #[test]
    fn random_sources_maintain_region_invariants(
        body in r#"[a-zA-Z0-9_ \t\n\r#"'`/\\{}\[\]();,=]*{0,200}"#
    ) {
        let index = SourceRegionIndex::build(&body);
        prop_assert!(region_invariants(&body, index.regions()));
        for region in index.regions() {
            prop_assert_ne!(region.kind, SourceRegionKind::Code);
        }
    }
}
