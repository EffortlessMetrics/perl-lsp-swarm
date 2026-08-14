use perl_parser_core::SourceLocation;
use perl_parser_core::quote_parser::{
    RegexFamilyOperator, extract_regex_family_geometry, extract_substitution_parts_strict,
};

#[test]
fn unpaired_substitution_replacement_ignores_delimiters_inside_quotes() -> Result<(), String> {
    let cases = [
        ("s/foo/\"a/b\"/ge", 20, "\"a/b\"", "ge", 26, 31, 32, 34),
        ("s/foo/'é/a'/u", 40, "'é/a'", "u", 46, 52, 53, 54),
    ];

    for (
        source,
        source_start,
        expected_replacement,
        expected_modifiers,
        replacement_start,
        replacement_end,
        modifier_start,
        modifier_end,
    ) in cases
    {
        let geometry = extract_regex_family_geometry(source, source_start)
            .ok_or_else(|| format!("missing geometry for {source:?}"))?;
        let replacement = geometry
            .replacement
            .as_ref()
            .ok_or_else(|| format!("missing replacement for {source:?}"))?;

        assert_eq!(geometry.operator, RegexFamilyOperator::Substitution);
        assert_eq!(geometry.pattern.text, "foo");
        assert_eq!(replacement.text, expected_replacement);
        assert_eq!(
            replacement.range,
            SourceLocation { start: replacement_start, end: replacement_end }
        );
        assert_eq!(
            replacement.opening_delimiter_range,
            SourceLocation { start: replacement_start - 1, end: replacement_start }
        );
        assert_eq!(
            replacement.closing_delimiter_range,
            Some(SourceLocation { start: replacement_end, end: replacement_end + 1 })
        );
        assert_eq!(geometry.modifiers.text, expected_modifiers);
        assert_eq!(
            geometry.modifiers.range,
            SourceLocation { start: modifier_start, end: modifier_end }
        );
        assert_eq!(geometry.full_range, SourceLocation { start: source_start, end: modifier_end });

        let parsed = extract_substitution_parts_strict(source).map_err(|error| {
            format!("strict substitution parse failed for {source:?}: {error:?}")
        })?;
        assert_eq!(
            parsed,
            ("foo".to_string(), expected_replacement.to_string(), geometry.modifiers.text)
        );
    }

    Ok(())
}

#[test]
fn geometry_rejects_identifier_prefixes_that_are_not_match_operators()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(extract_regex_family_geometry("match", 0).is_none());
    assert!(extract_regex_family_geometry("method", 0).is_none());
    Ok(())
}

#[test]
fn complete_geometry_matches_the_canonical_extractor_for_operator_forms()
-> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "/foo/i",
        "m/foo/i",
        "qr{foo}ms",
        "s/foo/bar/ge",
        "s/foo/'a/b'/r",
        "tr/a-z/A-Z/cd",
        "y{abc}{xyz}r",
    ] {
        assert!(
            extract_regex_family_geometry(source, 100).is_some(),
            "canonical extractor and geometry disagreed for {source:?}"
        );
    }
    Ok(())
}
