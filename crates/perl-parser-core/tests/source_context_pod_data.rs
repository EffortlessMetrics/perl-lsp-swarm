use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn pod_block_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "=pod\n\nParagraph\n\n=cut\nmy $x = 1;\n";
    let index = SourceRegionIndex::build(source);
    let pod_offset = source.find("Paragraph").ok_or("missing pod body")?;
    assert_eq!(index.kind_at_offset(pod_offset), SourceRegionKind::Pod);
    let code_offset = source.find("my").ok_or("missing code")?;
    assert_eq!(index.kind_at_offset(code_offset), SourceRegionKind::Code);
    Ok(())
}

#[test]
fn indented_pod_marker_is_not_pod() -> Result<(), Box<dyn std::error::Error>> {
    let source = "  =pod\nnot pod\n";
    let index = SourceRegionIndex::build(source);
    let pod_offset = source.find("=pod").ok_or("missing marker")?;
    assert_ne!(index.kind_at_offset(pod_offset), SourceRegionKind::Pod);
    Ok(())
}

#[test]
fn data_section_tail_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "1;\n__DATA__\nline one\nline two\n";
    let index = SourceRegionIndex::build(source);
    let data_offset = source.find("line one").ok_or("missing data line")?;
    assert_eq!(index.kind_at_offset(data_offset), SourceRegionKind::DataSection);
    Ok(())
}

#[test]
fn end_section_tail_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "1;\n__END__\ntail\n";
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.find("tail").ok_or("missing tail")?;
    assert_eq!(index.kind_at_offset(tail_offset), SourceRegionKind::DataSection);
    Ok(())
}
