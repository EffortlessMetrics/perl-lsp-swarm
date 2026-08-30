use perl_parser_core::syntax::source_context::{
    OffsetClassification, SourceRangeClassification, SourceRegionIndex, SourceRegionKind,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn typed_offset_classification_never_turns_invalid_input_into_code() -> TestResult {
    let source = "my $x = \"é\";\n";
    let index = SourceRegionIndex::build(source);
    let string_start = source.find('é').ok_or("fixture must contain the string character")?;

    assert_eq!(
        index.classify_offset(string_start),
        OffsetClassification::Proven { kind: SourceRegionKind::StringLiteral }
    );
    assert_eq!(
        index.classify_offset(string_start + 1),
        OffsetClassification::InvalidUtf8Boundary,
        "a UTF-8 continuation byte is not a source character boundary"
    );
    assert_eq!(
        index.classify_offset(source.len()),
        OffsetClassification::OutOfBounds,
        "EOF is a position boundary, not a source byte"
    );
    assert_eq!(
        index.classify_offset(source.len() + 1),
        OffsetClassification::OutOfBounds
    );
    Ok(())
}

#[test]
fn empty_ranges_report_both_sides_of_the_boundary() -> TestResult {
    let source = "my $x = 1; # note\n";
    let index = SourceRegionIndex::build(source);
    let comment_start = source.find('#').ok_or("fixture must contain a comment")?;

    assert_eq!(
        index.classify_range_checked(0, 0),
        SourceRangeClassification::EmptyBoundary {
            left: None,
            right: Some(SourceRegionKind::Code),
        }
    );
    assert_eq!(
        index.classify_range_checked(comment_start, comment_start),
        SourceRangeClassification::EmptyBoundary {
            left: Some(SourceRegionKind::Code),
            right: Some(SourceRegionKind::LineComment),
        },
        "a code-to-comment boundary must not silently select one side"
    );
    assert_eq!(
        index.classify_range_checked(source.len(), source.len()),
        SourceRangeClassification::EmptyBoundary {
            left: Some(SourceRegionKind::LineComment),
            right: None,
        },
        "EOF must stay a boundary rather than inherit the preceding region"
    );
    assert!(
        !index.range_fully_within(comment_start, comment_start, &[SourceRegionKind::LineComment]),
        "an empty boundary owns no bytes and cannot satisfy a full-range policy"
    );
    Ok(())
}

#[test]
fn empty_source_has_one_boundary_but_no_classifiable_byte() {
    let index = SourceRegionIndex::build("");

    assert_eq!(index.classify_offset(0), OffsetClassification::OutOfBounds);
    assert_eq!(
        index.classify_range_checked(0, 0),
        SourceRangeClassification::EmptyBoundary { left: None, right: None }
    );
}

#[test]
fn invalid_boundaries_and_interior_regions_remain_non_authoritative() -> TestResult {
    let source = "a \"é\" z";
    let index = SourceRegionIndex::build(source);
    let scalar_start = source.find('é').ok_or("fixture must contain a multibyte scalar")?;

    assert_eq!(
        index.classify_range_checked(scalar_start + 1, scalar_start + 1),
        SourceRangeClassification::InvalidUtf8Boundary
    );
    assert_eq!(
        index.classify_range_checked(source.len() + 1, source.len() + 1),
        SourceRangeClassification::OutOfBounds
    );
    assert_eq!(
        index.classify_range_checked(source.len(), 0),
        SourceRangeClassification::OutOfBounds
    );
    assert_eq!(
        index.classify_range_checked(0, source.len()),
        SourceRangeClassification::Ambiguous,
        "code-shaped endpoints must not hide the string region in the middle"
    );
    Ok(())
}
