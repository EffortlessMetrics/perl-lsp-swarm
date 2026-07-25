use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

#[test]
fn unclosed_double_quote_is_ambiguous_or_non_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"my $x = "unclosed"#;
    let index = SourceRegionIndex::build(source);
    let tail_offset = source.len().saturating_sub(1);
    let kind = index.kind_at_offset(tail_offset);
    assert!(
        matches!(
            kind,
            SourceRegionKind::StringLiteral | SourceRegionKind::RecoveryAmbiguous
        ),
        "unclosed literal should be non-code or recovery, got {kind:?}"
    );
    Ok(())
}
