use perl_parser_core::quote_parser::{
    RegexFamilyOperator, extract_regex_family_geometry, extract_substitution_parts_strict,
};

#[test]
fn unpaired_substitution_replacement_ignores_delimiters_inside_quotes()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "s/foo/\"a/b\"/ge";
    let geometry = extract_regex_family_geometry(source, 20).ok_or("missing geometry")?;
    let replacement = geometry.replacement.as_ref().ok_or("missing replacement")?;

    assert_eq!(geometry.operator, RegexFamilyOperator::Substitution);
    assert_eq!(geometry.pattern.text, "foo");
    assert_eq!(replacement.text, "\"a/b\"");
    assert_eq!(replacement.range.start, 26);
    assert_eq!(replacement.range.end, 31);
    assert_eq!(geometry.modifiers.text, "ge");
    assert_eq!(geometry.modifiers.range.start, 32);
    assert_eq!(geometry.modifiers.range.end, 34);
    assert_eq!(
        extract_substitution_parts_strict(source)?,
        ("foo".to_string(), "\"a/b\"".to_string(), "ge".to_string())
    );
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
