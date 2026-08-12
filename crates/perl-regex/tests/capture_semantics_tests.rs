use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    CaptureLanguageProfile, CaptureNumberConfidence, CaptureProfileConfidence,
    CaptureSourceConfidence, CaptureSyntax, EffectiveModifiers, FeatureState, ModifierSequence,
    PerlVersion, RegexLanguageProfile, RegexOperator,
};

fn profile(minor: u16, source_utf8: FeatureState) -> CaptureLanguageProfile {
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(
            Some(PerlVersion::new(5, minor)),
            FeatureState::Disabled,
        ),
        source_utf8,
    )
}

fn modifiers(raw: &str) -> Result<EffectiveModifiers, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new(raw, 0)
        .ok_or_else(|| format!("modifier range overflow for {raw:?}"))?;
    Ok(RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence,
        RegexLanguageProfile::new(
            Some(PerlVersion::new(5, 44)),
            FeatureState::Disabled,
        ),
    )
    .effective)
}

#[test]
fn mixed_group_forms_share_one_numbering_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(a)(?:b)(?<name>c)(?'other'd)(?P<py>e)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 4);
    assert_eq!(
        analysis
            .declarations
            .iter()
            .map(|declaration| declaration.number)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3), Some(4)]
    );
    assert_eq!(analysis.declarations[0].syntax, CaptureSyntax::Unnamed);
    assert_eq!(analysis.declarations[1].syntax, CaptureSyntax::NamedAngle);
    assert_eq!(analysis.declarations[2].syntax, CaptureSyntax::NamedQuote);
    assert_eq!(analysis.declarations[3].syntax, CaptureSyntax::PythonNamed);
    assert_eq!(analysis.declarations[0].group_range.start, 0);
    assert_eq!(analysis.declarations[0].group_range.end, 3);
    assert_eq!(analysis.declarations[0].body_range.start, 1);
    assert_eq!(analysis.declarations[0].body_range.end, 2);
    assert_eq!(
        pattern.get(
            analysis.declarations[1]
                .name_range
                .ok_or("missing name range")?
                .start
                ..analysis.declarations[1]
                    .name_range
                    .ok_or("missing name range")?
                    .end
        ),
        Some("name")
    );
    assert_eq!(
        analysis.named_families.iter().map(|family| family.name.as_str()).collect::<Vec<_>>(),
        vec!["name", "other", "py"]
    );
    assert!(analysis.status.is_complete());
    assert!(analysis.diagnostics.is_empty());

    let legacy = RegexAnalyzer::extract_named_captures(pattern);
    assert_eq!(
        legacy
            .iter()
            .map(|capture| (capture.name.as_str(), capture.index, capture.pattern.as_str()))
            .collect::<Vec<_>>(),
        vec![("name", 2, "c"), ("other", 3, "d"), ("py", 4, "e")]
    );
    Ok(())
}

#[test]
fn n_disables_ordinary_captures_but_local_minus_n_reenables_them()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(a)(?<name>b)(?-n:(c))";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        modifiers("n")?,
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 2);
    assert_eq!(analysis.declarations[0].name.as_deref(), Some("name"));
    assert_eq!(analysis.declarations[0].number, Some(1));
    assert_eq!(analysis.declarations[1].name, None);
    assert_eq!(analysis.declarations[1].number, Some(2));
    assert_eq!(
        pattern.get(
            analysis.declarations[1].body_range.start..analysis.declarations[1].body_range.end
        ),
        Some("c")
    );
    Ok(())
}

#[test]
fn branch_reset_restarts_each_branch_and_advances_by_the_widest_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?|(a)(?<x>b)|(c))(?<after>d)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(
        analysis
            .declarations
            .iter()
            .map(|declaration| declaration.number)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(1), Some(3)]
    );
    assert_eq!(analysis.declarations[1].name.as_deref(), Some("x"));
    assert_eq!(analysis.declarations[3].name.as_deref(), Some("after"));
    assert_eq!(
        analysis.named_families.iter().map(|family| family.name.as_str()).collect::<Vec<_>>(),
        vec!["x", "after"]
    );
    Ok(())
}

#[test]
fn duplicate_names_preserve_every_declaration_and_numbered_alias()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?<x>a)(?<x>b)(?<y>c)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 3);
    assert_eq!(analysis.named_families.len(), 2);
    assert_eq!(analysis.named_families[0].name, "x");
    assert_eq!(
        analysis.named_families[0]
            .declarations
            .iter()
            .map(|id| id.index())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(analysis.declarations[0].number, Some(1));
    assert_eq!(analysis.declarations[1].number, Some(2));
    assert_eq!(analysis.named_families[1].name, "y");
    Ok(())
}

#[test]
fn interpolation_makes_only_later_capture_numbers_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?<before>a)$runtime(?<after>b)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert!(analysis.status.dynamic);
    assert_eq!(analysis.declarations[0].number, Some(1));
    assert_eq!(
        analysis.declarations[0].confidence.number,
        CaptureNumberConfidence::Exact
    );
    assert_eq!(analysis.declarations[1].number, None);
    assert_eq!(
        analysis.declarations[1].confidence.number,
        CaptureNumberConfidence::DynamicUnknown
    );
    Ok(())
}

#[test]
fn deferred_runtime_regex_text_is_a_numbering_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?<before>a)(??{ build() })(?<after>b)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert!(analysis.status.dynamic);
    assert_eq!(analysis.declarations[0].number, Some(1));
    assert_eq!(analysis.declarations[1].number, None);
    assert_eq!(
        analysis.declarations[1].confidence.number,
        CaptureNumberConfidence::DynamicUnknown
    );
    Ok(())
}

#[test]
fn invalid_names_diagnose_without_inventing_capture_declarations()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?<>a)(?<1bad>b)(?<bad-name>c)(?<good>d)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code
                    == perl_regex::analyzer::CaptureDiagnosticCode::InvalidName
            })
            .count(),
        3
    );
    assert_eq!(analysis.declarations.len(), 1);
    assert_eq!(analysis.declarations[0].name.as_deref(), Some("good"));
    assert_eq!(analysis.declarations[0].number, None);
    assert_eq!(
        analysis.declarations[0].confidence.number,
        CaptureNumberConfidence::StructuralUnknown
    );
    assert!(analysis.status.malformed);
    Ok(())
}

#[test]
fn old_perl_profiles_retain_the_fact_but_mark_it_incompatible()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_captures(
        "(?<name>a)",
        EffectiveModifiers::default(),
        profile(8, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 1);
    assert_eq!(
        analysis.declarations[0].confidence.profile,
        CaptureProfileConfidence::Incompatible
    );
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].code,
        perl_regex::analyzer::CaptureDiagnosticCode::RequiresPerlVersion
    );
    assert_eq!(analysis.diagnostics[0].required_perl_version, Some((5, 10)));
    Ok(())
}

#[test]
fn unicode_names_require_utf8_and_keep_unmodeled_continuations_profile_dependent()
-> Result<(), Box<dyn std::error::Error>> {
    let exact = RegexAnalyzer::analyze_captures(
        "(?<名>a)",
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );
    assert_eq!(exact.declarations.len(), 1);
    assert_eq!(
        exact.declarations[0].confidence.profile,
        CaptureProfileConfidence::Exact
    );

    let disabled = RegexAnalyzer::analyze_captures(
        "(?<名>a)",
        EffectiveModifiers::default(),
        profile(44, FeatureState::Disabled),
    );
    assert_eq!(
        disabled.declarations[0].confidence.profile,
        CaptureProfileConfidence::Incompatible
    );
    assert_eq!(
        disabled.diagnostics[0].code,
        perl_regex::analyzer::CaptureDiagnosticCode::RequiresSourceUtf8
    );

    let unknown = RegexAnalyzer::analyze_captures(
        "(?<名>a)",
        EffectiveModifiers::default(),
        CaptureLanguageProfile::unknown(),
    );
    assert_eq!(
        unknown.declarations[0].confidence.profile,
        CaptureProfileConfidence::ProfileDependent
    );

    let combining = RegexAnalyzer::analyze_captures(
        "(?<e\u{301}>a)",
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );
    assert_eq!(combining.declarations.len(), 1);
    assert_eq!(
        combining.declarations[0].confidence.profile,
        CaptureProfileConfidence::ProfileDependent
    );
    assert!(combining.diagnostics.is_empty());
    Ok(())
}

#[test]
fn excluded_regions_do_not_create_capture_declarations()
-> Result<(), Box<dyn std::error::Error>> {
    // `(?#...)` ends at the first unescaped `)`. The fake named-group opener is
    // therefore comment text; a second closing paren would be unmatched source.
    let pattern = r"\Q(?<quoted>x)\E[(?<class>y)](?#(?<comment>z)(?<real>a)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 1);
    assert_eq!(analysis.declarations[0].name.as_deref(), Some("real"));
    assert_eq!(analysis.declarations[0].number, Some(1));
    Ok(())
}

#[test]
fn nested_group_bodies_use_the_matching_close_not_the_first_close()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r"(?<outer>(inner)\d+)";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 2);
    assert_eq!(analysis.declarations[0].name.as_deref(), Some("outer"));
    assert_eq!(analysis.declarations[0].number, Some(1));
    assert_eq!(analysis.declarations[1].number, Some(2));
    assert_eq!(
        pattern.get(
            analysis.declarations[0].body_range.start..analysis.declarations[0].body_range.end
        ),
        Some(r"(inner)\d+")
    );

    let legacy = RegexAnalyzer::extract_named_captures(pattern);
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].name, "outer");
    assert_eq!(legacy[0].index, 1);
    assert_eq!(legacy[0].pattern, r"(inner)\d+");
    Ok(())
}

#[test]
fn unclosed_groups_are_retained_as_recovered_and_filtered_from_legacy_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?<name>a";
    let analysis = RegexAnalyzer::analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.declarations.len(), 1);
    assert_eq!(
        analysis.declarations[0].confidence.source,
        CaptureSourceConfidence::Recovered
    );
    assert!(analysis.status.malformed);
    assert!(RegexAnalyzer::extract_named_captures(pattern).is_empty());
    Ok(())
}
