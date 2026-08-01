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
    // Assert the positive classification, not merely `!= LineComment`. An
    // uncovered offset falls back to `Code`, which also satisfies `!=
    // LineComment` — so the weaker oracle passed while every double-quoted
    // string was in fact unclassified.
    assert_eq!(
        index.kind_at_offset(hash_offset),
        SourceRegionKind::StringLiteral,
        "hash inside a double-quoted string must classify as the string literal"
    );
    Ok(())
}

/// `PerlLexer` emits `InterpolatedString` for every double-quoted string —
/// interpolating or not — so a collector that only maps `StringLiteral` leaves
/// the whole `"…"` span uncovered and `kind_at_offset` reports `Code`.
#[test]
fn double_quoted_strings_classify_as_string_literal() -> Result<(), Box<dyn std::error::Error>> {
    for source in ["my $x = \"hello world\";\n", "my $x = \"hello $name world\";\n"] {
        let index = SourceRegionIndex::build(source);
        let inner = source.find("hello").ok_or("missing body")?;
        assert_eq!(
            index.kind_at_offset(inner),
            SourceRegionKind::StringLiteral,
            "double-quoted body must classify as StringLiteral in {source:?}"
        );
    }
    Ok(())
}

/// Single- and double-quoted strings must agree; the asymmetry was the defect.
#[test]
fn single_and_double_quoted_agree() -> Result<(), Box<dyn std::error::Error>> {
    let single = SourceRegionIndex::build("my $x = 'hello world';\n");
    let double = SourceRegionIndex::build("my $x = \"hello world\";\n");
    assert_eq!(single.kind_at_offset(9), double.kind_at_offset(9));
    assert_eq!(single.kind_at_offset(9), SourceRegionKind::StringLiteral);
    Ok(())
}

/// An unterminated literal must stay `RecoveryAmbiguous`, not decay to `Code`.
/// `SourceRegionKind::RecoveryAmbiguous` documents this fail-closed contract;
/// the lexer reports the span as `Error`, which the collector was discarding.
#[test]
fn unterminated_literal_is_recovery_ambiguous() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = \"open\n";
    let index = SourceRegionIndex::build(source);
    let quote = source.find('"').ok_or("missing quote")?;
    assert_eq!(
        index.kind_at_offset(quote),
        SourceRegionKind::RecoveryAmbiguous,
        "unterminated literal must not read as executable code"
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
