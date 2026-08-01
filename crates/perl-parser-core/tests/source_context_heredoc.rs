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

#[test]
fn indented_heredoc_body_classified() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = <<~EOF;\n    body line\n    EOF\nmy $y = 1;\n";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find("body").ok_or("missing heredoc body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::Heredoc);
    let code_offset = source.find("my $y").ok_or("missing trailing code")?;
    assert_eq!(index.kind_at_offset(code_offset), SourceRegionKind::Code);
    Ok(())
}

#[test]
fn left_shift_expression_is_not_a_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = 8;\nmy $y = $x << 2;\nmy $z = 3;\n";
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.find("my $z").ok_or("missing trailing code")?;
    assert_eq!(
        index.kind_at_offset(tail_offset),
        SourceRegionKind::Code,
        "`$x << 2` must not open a heredoc, regions: {:?}",
        index.regions()
    );
    Ok(())
}

#[test]
fn numeric_label_does_not_open_heredoc() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $y = 1 << 32;\nmy $z = 3;\n";
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.find("my $z").ok_or("missing trailing code")?;
    assert_eq!(index.kind_at_offset(tail_offset), SourceRegionKind::Code);
    Ok(())
}
