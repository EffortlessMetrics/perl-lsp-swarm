//! Overlap resolution and UTF-8 boundary regressions for the source region index.
//!
//! These cover defects found in review of #5003 PR1: nested higher-precedence
//! regions used to replace their enclosing region outright (losing the enclosing
//! prefix and suffix), overrides were never overlap-resolved, and range/recovery
//! offsets could land mid-codepoint.

use perl_parser_core::{RangeClassification, SourceRegion, SourceRegionIndex, SourceRegionKind};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A heredoc opener inside a POD block nests a higher-precedence region inside a
/// lower-precedence one. The POD prefix before it and the POD suffix after it
/// must both keep their kind instead of collapsing to `Code`.
#[test]
fn nested_higher_precedence_region_keeps_enclosing_prefix_and_suffix() -> TestResult {
    let source = "=pod\n\ntext <<EOF here\nbody\nEOF\nmore pod\n\n=cut\nmy $x = 1;\n";
    let index = SourceRegionIndex::build(source);

    let prefix_offset = source.find("text").ok_or("missing pod prefix")?;
    assert_eq!(
        index.kind_at_offset(prefix_offset),
        SourceRegionKind::Pod,
        "pod prefix before the nested heredoc must stay pod, regions: {:?}",
        index.regions()
    );

    let suffix_offset = source.find("more pod").ok_or("missing pod suffix")?;
    assert_eq!(
        index.kind_at_offset(suffix_offset),
        SourceRegionKind::Pod,
        "pod suffix after the nested heredoc must stay pod, regions: {:?}",
        index.regions()
    );

    let code_offset = source.find("my $x").ok_or("missing trailing code")?;
    assert_eq!(index.kind_at_offset(code_offset), SourceRegionKind::Code);
    Ok(())
}

/// A heredoc opener inside `__DATA__` payload must not swallow the data section:
/// the marker itself and the tail after the heredoc stay `DataSection`.
#[test]
fn nested_heredoc_in_data_section_keeps_marker_and_tail() -> TestResult {
    let source = "my $z = 1;\n__DATA__\nsome <<EOF text\nbody\nEOF\ntail data\n";
    let index = SourceRegionIndex::build(source);

    let marker_offset = source.find("__DATA__").ok_or("missing data marker")?;
    assert_eq!(
        index.kind_at_offset(marker_offset),
        SourceRegionKind::DataSection,
        "data marker must stay data_section, regions: {:?}",
        index.regions()
    );

    let tail_offset = source.find("tail data").ok_or("missing data tail")?;
    assert_eq!(
        index.kind_at_offset(tail_offset),
        SourceRegionKind::DataSection,
        "data tail must stay data_section, regions: {:?}",
        index.regions()
    );
    Ok(())
}

/// The stored region list is documented as sorted and non-overlapping; overrides
/// that overlap existing regions must be resolved, not appended verbatim.
#[test]
fn overlapping_overrides_are_resolved_into_non_overlapping_regions() -> TestResult {
    let source = "my $x = 1; # trailing comment\n";
    let index = SourceRegionIndex::build(source);
    let override_region = SourceRegion::new(0, source.len(), SourceRegionKind::Pod)
        .ok_or("override region should construct")?;

    let overridden = index.with_overrides(&[override_region]);
    let regions = overridden.regions();
    for pair in regions.windows(2) {
        assert!(
            pair[0].end <= pair[1].start,
            "overrides must not leave overlapping regions, got: {regions:?}"
        );
    }
    assert!(
        regions.iter().all(|region| region.end <= source.len()),
        "regions must stay in bounds: {regions:?}"
    );
    Ok(())
}

/// `classify_range` inspected `end - 1`, which lands on a UTF-8 continuation
/// byte when the range ends with a multibyte character.
#[test]
fn classify_range_ending_on_multibyte_char_is_proven() -> TestResult {
    // No trailing newline: the comment region ends immediately after `é`, so the
    // `end - 1` probe lands on a UTF-8 continuation byte.
    let source = "my $x = 1; # café";
    let index = SourceRegionIndex::build(source);
    let comment_start = source.find('#').ok_or("missing comment")?;
    let comment_end = source.len();

    assert_eq!(
        index.classify_range(comment_start, comment_end),
        RangeClassification::Proven { kind: SourceRegionKind::LineComment },
        "range ending on a multibyte char must stay proven, regions: {:?}",
        index.regions()
    );
    assert!(index.range_fully_within(comment_start, comment_end, &[SourceRegionKind::LineComment]));
    Ok(())
}

/// Every stored region must start and end on a UTF-8 char boundary, including
/// the recovery region emitted for a literal left open at EOF.
#[test]
fn recovery_region_lands_on_char_boundary() -> TestResult {
    let source = "my $x = \"hé";
    let index = SourceRegionIndex::build(source);
    let regions = index.regions();
    for region in regions {
        assert!(
            source.is_char_boundary(region.start) && source.is_char_boundary(region.end),
            "region must lie on char boundaries: {region:?} in {regions:?}"
        );
    }

    let accented = source.rfind('é').ok_or("missing multibyte char")?;
    assert_ne!(
        index.kind_at_offset(accented),
        SourceRegionKind::Code,
        "unterminated literal tail must not classify as code, regions: {regions:?}"
    );
    Ok(())
}
