use perl_parser_core::{
    ParseError, Parser, SourceLocation,
    quote_parser::{RegexFamilyGeometry, RegexFamilyOperator, extract_regex_family_geometry},
};

fn geometry(
    source: &str,
    source_start: usize,
) -> Result<RegexFamilyGeometry, Box<dyn std::error::Error>> {
    extract_regex_family_geometry(source, source_start)
        .ok_or_else(|| format!("expected regex-family geometry for {source:?}").into())
}

fn absolute(source_start: usize, start: usize, end: usize) -> SourceLocation {
    SourceLocation { start: source_start + start, end: source_start + end }
}

#[test]
fn regex_operator_geometry_preserves_exact_prefix_body_and_modifier_ranges()
-> Result<(), Box<dyn std::error::Error>> {
    let source_start = 100;
    let cases = [
        (
            "/foo/",
            RegexFamilyOperator::BareMatch,
            absolute(source_start, 0, 0),
            absolute(source_start, 0, 1),
            absolute(source_start, 1, 4),
            absolute(source_start, 4, 5),
            absolute(source_start, 5, 5),
            absolute(source_start, 0, 5),
        ),
        (
            "m/foo/i",
            RegexFamilyOperator::Match,
            absolute(source_start, 0, 1),
            absolute(source_start, 1, 2),
            absolute(source_start, 2, 5),
            absolute(source_start, 5, 6),
            absolute(source_start, 6, 7),
            absolute(source_start, 0, 7),
        ),
        (
            "qr{foo}ms",
            RegexFamilyOperator::QuoteRegex,
            absolute(source_start, 0, 2),
            absolute(source_start, 2, 3),
            absolute(source_start, 3, 6),
            absolute(source_start, 6, 7),
            absolute(source_start, 7, 9),
            absolute(source_start, 0, 9),
        ),
        (
            "m # note\n {foo}x",
            RegexFamilyOperator::Match,
            absolute(source_start, 0, 1),
            absolute(source_start, 10, 11),
            absolute(source_start, 11, 14),
            absolute(source_start, 14, 15),
            absolute(source_start, 15, 16),
            absolute(source_start, 0, 16),
        ),
    ];

    for (source, operator, operator_range, open, body, close, modifiers, full) in cases {
        let result = geometry(source, source_start)?;
        assert_eq!(result.operator, operator, "operator for {source:?}");
        assert_eq!(result.operator_range, operator_range, "operator range for {source:?}");
        assert_eq!(
            result.pattern.opening_delimiter_range, open,
            "opening delimiter for {source:?}"
        );
        assert_eq!(result.pattern.range, body, "body range for {source:?}");
        assert_eq!(
            result.pattern.closing_delimiter_range,
            Some(close),
            "closing delimiter for {source:?}"
        );
        assert_eq!(result.modifiers.range, modifiers, "modifier range for {source:?}");
        assert_eq!(result.full_range, full, "full range for {source:?}");
        assert!(result.replacement.is_none(), "unexpected replacement for {source:?}");
    }

    Ok(())
}

#[test]
fn two_body_operator_geometry_keeps_pattern_replacement_and_modifier_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let source_start = 40;
    let cases = [
        (
            "s/foo/bar/ge",
            RegexFamilyOperator::Substitution,
            absolute(source_start, 2, 5),
            absolute(source_start, 6, 9),
            absolute(source_start, 10, 12),
        ),
        (
            "s{foo}[bar]r",
            RegexFamilyOperator::Substitution,
            absolute(source_start, 2, 5),
            absolute(source_start, 7, 10),
            absolute(source_start, 11, 12),
        ),
        (
            "tr/a-z/A-Z/cd",
            RegexFamilyOperator::Transliteration,
            absolute(source_start, 3, 6),
            absolute(source_start, 7, 10),
            absolute(source_start, 11, 13),
        ),
        (
            "y{abc}{xyz}r",
            RegexFamilyOperator::TransliterationAlias,
            absolute(source_start, 2, 5),
            absolute(source_start, 7, 10),
            absolute(source_start, 11, 12),
        ),
    ];

    for (source, operator, pattern_range, replacement_range, modifier_range) in cases {
        let result = geometry(source, source_start)?;
        let replacement = result
            .replacement
            .as_ref()
            .ok_or_else(|| format!("expected replacement geometry for {source:?}"))?;
        assert_eq!(result.operator, operator, "operator for {source:?}");
        assert_eq!(result.pattern.range, pattern_range, "pattern range for {source:?}");
        assert_eq!(replacement.range, replacement_range, "replacement range for {source:?}");
        assert_eq!(result.modifiers.range, modifier_range, "modifier range for {source:?}");
    }

    let shared = geometry("s/foo/bar/ge", source_start)?;
    let shared_replacement = shared.replacement.as_ref().ok_or("missing replacement")?;
    assert_eq!(shared.pattern.closing_delimiter_range, Some(absolute(source_start, 5, 6)));
    assert_eq!(
        shared_replacement.opening_delimiter_range,
        absolute(source_start, 5, 6),
        "the middle slash closes the pattern and opens the replacement"
    );
    assert_eq!(shared_replacement.closing_delimiter_range, Some(absolute(source_start, 9, 10)));

    Ok(())
}

#[test]
fn geometry_preserves_partial_and_unicode_byte_ranges() -> Result<(), Box<dyn std::error::Error>> {
    let source_start = 7;

    let partial = geometry("m{foo", source_start)?;
    assert_eq!(partial.pattern.text, "foo");
    assert_eq!(partial.pattern.range, absolute(source_start, 2, 5));
    assert!(!partial.pattern.is_closed());

    let missing_replacement_closer = geometry("s/foo/", source_start)?;
    let replacement =
        missing_replacement_closer.replacement.as_ref().ok_or("expected partial replacement")?;
    assert_eq!(replacement.text, "");
    assert_eq!(replacement.range, absolute(source_start, 6, 6));
    assert!(!replacement.is_closed());

    let unicode_source = "qr{é(?<名>.)}u";
    let unicode = geometry(unicode_source, source_start)?;
    assert_eq!(unicode.pattern.text, "é(?<名>.)");
    assert_eq!(unicode.pattern.range, absolute(source_start, 3, 14));
    assert_eq!(unicode.pattern.closing_delimiter_range, Some(absolute(source_start, 14, 15)));
    assert_eq!(unicode.modifiers.text, "u");
    assert_eq!(unicode.modifiers.range, absolute(source_start, 15, 16));

    Ok(())
}

#[test]
fn nested_quantifier_advisories_use_original_pattern_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let sources =
        ["my $x = /(a+)+/;", "my $x = m/(a+)+/;", "my $x = qr{(a+)+};", "my $x = s/(a+)+/x/;"];

    for source in sources {
        let pattern_start = source
            .find("(a+)+")
            .ok_or_else(|| format!("fixture lost nested pattern: {source:?}"))?;
        let expected = pattern_start + 4;
        let mut parser = Parser::new(source);
        let _ast = parser.parse()?;
        let actual = parser
            .get_errors()
            .iter()
            .find_map(|error| match error {
                ParseError::Advisory { message, location }
                    if message.contains("Nested quantifiers") =>
                {
                    Some(*location)
                }
                _ => None,
            })
            .ok_or_else(|| format!("expected nested-quantifier advisory for {source:?}"))?;
        assert_eq!(actual, expected, "wrong source coordinate for {source:?}");
    }

    Ok(())
}
