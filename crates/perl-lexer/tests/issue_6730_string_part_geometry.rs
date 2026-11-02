//! Authoritative `StringPart` source geometry (#6730).

use perl_lexer::{PerlLexer, StringPartKind, TokenType};

type R = Result<(), Box<dyn std::error::Error>>;

fn interpolated_parts(
    input: &str,
) -> Result<Vec<perl_lexer::StringPart>, Box<dyn std::error::Error>> {
    let tok = PerlLexer::new(input).next_token().ok_or("missing token")?;
    match tok.token_type {
        TokenType::InterpolatedString(parts) => Ok(parts),
        other => Err(format!("expected InterpolatedString, got {other:?}").into()),
    }
}

#[test]
fn segment_text_matches_source_slice() -> R {
    let input = r#""a$x$x""#;
    let tok = PerlLexer::new(input).next_token().ok_or("missing token")?;
    let parts = interpolated_parts(input)?;
    for part in &parts {
        assert_eq!(
            part.text(),
            &input[part.start()..part.end()],
            "part {:?} must match its source slice",
            part
        );
        assert!(part.start() >= tok.start && part.end() <= tok.end);
    }
    Ok(())
}

#[test]
fn repeated_identical_variables_get_distinct_ranges() -> R {
    let parts = interpolated_parts(r#""$x$x""#)?;
    let vars: Vec<_> =
        parts.iter().filter(|p| p.kind() == StringPartKind::Variable && p.text() == "$x").collect();
    assert_eq!(vars.len(), 2, "expected two '$x' parts, got {parts:?}");
    assert_ne!(vars[0].start(), vars[1].start(), "duplicate text must not collapse to one range");
    Ok(())
}

#[test]
fn expression_tail_uses_lexer_range_not_search() -> R {
    let input = r#""$a[0]""#;
    let parts = interpolated_parts(input)?;
    let slice = parts
        .iter()
        .find(|p| p.kind() == StringPartKind::ArraySlice)
        .ok_or("missing ArraySlice part")?;
    assert_eq!(slice.text(), "[0]");
    assert_eq!(&input[slice.start()..slice.end()], "[0]");
    Ok(())
}
