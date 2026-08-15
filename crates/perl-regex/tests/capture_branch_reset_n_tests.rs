use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    CaptureLanguageProfile, EffectiveModifiers, FeatureState, ModifierSequence, PerlVersion,
    RegexLanguageProfile, RegexOperator,
};

fn modifiers_n() -> Result<EffectiveModifiers, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new("n", 0).ok_or("modifier range overflow")?;
    Ok(RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence,
        RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Disabled),
    )
    .effective)
}

fn profile() -> CaptureLanguageProfile {
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Disabled),
        FeatureState::Enabled,
    )
}

#[test]
fn branch_reset_respects_n_and_local_minus_n_per_alternative()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?|(?<x>a)(b)|(?-n:(c)))(?<after>d)";
    let analysis = RegexAnalyzer::analyze_captures(pattern, modifiers_n()?, profile());

    assert_eq!(analysis.declarations.len(), 3);
    assert_eq!(
        analysis
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_deref(), declaration.number))
            .collect::<Vec<_>>(),
        vec![(Some("x"), Some(1)), (None, Some(1)), (Some("after"), Some(2))]
    );

    let suppressed_body = pattern.find("(b)").ok_or("missing suppressed group")?;
    assert!(
        analysis
            .declarations
            .iter()
            .all(|declaration| declaration.group_range.start != suppressed_body)
    );
    assert!(analysis.status.is_complete());
    assert!(analysis.diagnostics.is_empty());
    Ok(())
}
