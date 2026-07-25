use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn bare_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<EOF;\nbody line\nEOF\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    Ok(())
}

#[test]
fn quoted_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<'EOF';\nbody line\nEOF\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    Ok(())
}
