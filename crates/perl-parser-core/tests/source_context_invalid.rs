use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn unclosed_double_quote_is_ambiguous_or_non_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"my $x = "unclosed"#;
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.len().saturating_sub(1);
    let kind = index.kind_at_offset(tail_offset);
    assert!(
        matches!(kind, SourceRegionKind::StringLiteral | SourceRegionKind::RecoveryAmbiguous),
        "unclosed literal should be non-code or recovery, got {kind:?}"
    );
    Ok(())
}

/// A reversed range is not a valid span. Reporting it as contained would let a
/// caller treat a malformed query as proven coverage, contradicting
/// `SourceRegion::new` and `SourceRegionIndex::classify_range`, which both
/// reject the same ordering.
#[test]
fn contains_range_rejects_reversed_span() -> Result<(), Box<dyn std::error::Error>> {
    let source = "# comment\n";
    let index = SourceRegionIndex::build(source);
    let region = *index.regions().first().ok_or("missing comment region")?;
    assert!(region.contains_range(2, 5), "forward subrange must be contained");
    assert!(!region.contains_range(5, 2), "reversed range must never be contained");
    Ok(())
}
