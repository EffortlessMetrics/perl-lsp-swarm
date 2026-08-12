use perl_regex::analyzer::{EffectiveModifiers, ExtendedMode};
use perl_regex::validator::RegexValidator;

#[test]
fn extended_trivia_does_not_hide_a_nested_quantifier() {
    let modifiers = EffectiveModifiers {
        extended: ExtendedMode::Extended,
        ..EffectiveModifiers::default()
    };
    let analysis = RegexValidator::new().analyze_with_modifiers(
        "(a+) # gap\n +",
        modifiers,
    );

    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(analysis.facts.nested_quantifiers[0].start, 12);
}
