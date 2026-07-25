use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn code_vs_line_comment() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = 1; # trailing\n";
    let index = SourceRegionIndex::build(source);
    let hash_offset = source.find('#').ok_or("missing hash")?;
    assert_eq!(index.kind_at_offset(hash_offset), SourceRegionKind::LineComment);
    assert_eq!(index.kind_at_offset(0), SourceRegionKind::Code);
    Ok(())
}

#[test]
fn hash_inside_double_quotes_is_not_comment() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = \"# not a comment\";";
    let index = SourceRegionIndex::build(source);
    let hash_offset = source.find('#').ok_or("missing hash")?;
    assert_ne!(
        index.kind_at_offset(hash_offset),
        SourceRegionKind::LineComment,
        "hash inside string must not classify as line comment"
    );
    Ok(())
}

#[test]
fn hash_inside_single_quotes_is_not_comment() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = '# not a comment';";
    let index = SourceRegionIndex::build(source);
    let hash_offset = source.find('#').ok_or("missing hash")?;
    assert_ne!(index.kind_at_offset(hash_offset), SourceRegionKind::LineComment);
    Ok(())
}

#[test]
fn hash_inside_backticks_is_not_comment() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = `# not a comment`;";
    let index = SourceRegionIndex::build(source);
    let hash_offset = source.find('#').ok_or("missing hash")?;
    assert_ne!(index.kind_at_offset(hash_offset), SourceRegionKind::LineComment);
    Ok(())
}

#[test]
fn classify_range_proven_for_uniform_comment() -> Result<(), Box<dyn std::error::Error>> {
    use perl_parser_core::RangeClassification;
    let source = "# only comment\n";
    let index = SourceRegionIndex::build(source);
    match index.classify_range(0, source.len()) {
        RangeClassification::Proven { kind } => {
            assert_eq!(kind, SourceRegionKind::LineComment);
        }
        other => return Err(format!("expected proven comment, got {other:?}").into()),
    }
    Ok(())
}
