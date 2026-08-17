use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    EffectiveModifiers, ExtendedMode, FeatureState, ModifierSequence, PerlVersion,
    RegexLanguageProfile, RegexOperator,
};
use perl_regex::validator::RegexValidator;

/// `EffectiveModifiers` is `#[non_exhaustive]`, so effective state is derived
/// through the modifier analyzer rather than built from a struct literal.
fn effective(raw: &str) -> Result<EffectiveModifiers, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new(raw, 0)
        .ok_or_else(|| format!("modifier range overflow for {raw:?}"))?;
    Ok(RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence,
        RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Disabled),
    )
    .effective)
}

#[test]
fn extended_trivia_does_not_hide_a_nested_quantifier() -> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective("x")?;
    assert_eq!(modifiers.extended, ExtendedMode::Extended);

    let analysis = RegexValidator::new().analyze_with_modifiers("(a+) # gap\n +", modifiers);

    assert_eq!(
        analysis.facts.nested_quantifiers.len(),
        1,
        "extended-mode trivia hid the nested quantifier: {:?}",
        analysis.facts.nested_quantifiers
    );
    assert_eq!(analysis.facts.nested_quantifiers[0].start, 12);
    Ok(())
}
