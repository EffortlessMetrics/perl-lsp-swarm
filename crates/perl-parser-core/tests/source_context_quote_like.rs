use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn q_brace_body_is_quote_like() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = q{body};";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find('b').ok_or("missing body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::QuoteLike);
    Ok(())
}

#[test]
fn qq_slash_body_is_quote_like() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = qq/body/;";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find('b').ok_or("missing body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::QuoteLike);
    Ok(())
}

#[test]
fn qr_slash_body_is_regex_literal() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $re = qr/pattern/;";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find('p').ok_or("missing pattern")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::RegexLike);
    Ok(())
}

#[test]
fn m_bang_body_is_regex_literal() -> Result<(), Box<dyn std::error::Error>> {
    let source = "m!body!";
    let index = SourceRegionIndex::build(source);
    let body_offset = source.find('b').ok_or("missing body")?;
    assert_eq!(index.kind_at_offset(body_offset), SourceRegionKind::RegexLike);
    Ok(())
}

#[test]
fn substitution_body_is_regex_literal() -> Result<(), Box<dyn std::error::Error>> {
    let source = "s/foo/bar/";
    let index = SourceRegionIndex::build(source);
    let foo_offset = source.find('f').ok_or("missing foo")?;
    assert_eq!(index.kind_at_offset(foo_offset), SourceRegionKind::RegexLike);
    Ok(())
}

#[test]
fn tr_body_is_regex_literal() -> Result<(), Box<dyn std::error::Error>> {
    let source = "tr/a/b/";
    let index = SourceRegionIndex::build(source);
    let first_a = source.find('a').ok_or("missing a")?;
    assert_eq!(index.kind_at_offset(first_a), SourceRegionKind::RegexLike);
    Ok(())
}
