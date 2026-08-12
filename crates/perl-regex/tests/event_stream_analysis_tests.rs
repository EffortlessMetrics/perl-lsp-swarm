use perl_regex::{RegexAnalyzer, RegexValidator};
use perl_regex::analyzer::{
    FeatureState, ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator,
};
use perl_regex::validator::{RegexDiagnosticCode, RegexValidationConfig};

fn effective(
    operator: RegexOperator,
    raw: &str,
) -> Result<perl_regex::analyzer::EffectiveModifiers, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new(raw, 0)
        .ok_or_else(|| format!("modifier range overflow for {raw:?}"))?;
    let profile = RegexLanguageProfile::new(
        Some(PerlVersion::new(5, 44)),
        FeatureState::Enabled,
    );
    Ok(RegexAnalyzer::analyze_modifiers(operator, sequence, profile).effective)
}

#[test]
fn extended_line_comments_are_excluded_from_every_analysis()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "# (?{ hidden }) (a+)+ \\p{L}\\p{N}\n(a+)+";
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze_with_modifiers(
        pattern,
        effective(RegexOperator::Match, "x")?,
    );

    assert!(analysis.facts.embedded_code.is_empty());
    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == RegexDiagnosticCode::UnicodePropertyLimit)
            .count(),
        0
    );
    assert_eq!(
        analysis.facts.nested_quantifiers[0].start,
        pattern.rfind('+').ok_or("missing live outer quantifier")?
    );
    Ok(())
}

#[test]
fn group_comments_are_excluded_before_later_live_structure()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = "(?# (?{ hidden })(a+)+";
    let analysis = RegexValidator::new().analyze(pattern);

    assert!(analysis.facts.embedded_code.is_empty());
    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(
        analysis.facts.nested_quantifiers[0].start,
        pattern.rfind('+').ok_or("missing live outer quantifier")?
    );
    Ok(())
}

#[test]
fn local_x_scope_hides_structure_and_minus_x_restores_literal_hash_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let locally_extended = "(?x:# (?{ hidden }) (a+)+\n(a+)+)";
    let extended = validator.analyze(locally_extended);
    assert!(extended.facts.embedded_code.is_empty());
    assert_eq!(extended.facts.nested_quantifiers.len(), 1);
    assert_eq!(
        extended.facts.nested_quantifiers[0].start,
        locally_extended.rfind('+').ok_or("missing live local quantifier")?
    );

    let locally_literal = "(?-x:#(?{ live }))";
    let literal = validator.analyze_with_modifiers(
        locally_literal,
        effective(RegexOperator::Match, "x")?,
    );
    assert_eq!(literal.facts.embedded_code.len(), 1);
    assert_eq!(
        literal.facts.embedded_code[0].range.start,
        locally_literal.find("(?{").ok_or("missing embedded-code opener")?
    );
    Ok(())
}

#[test]
fn optional_exact_and_repeating_outer_quantifiers_are_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let safe = ["(a+)?", "(a+){0,1}", "(a+){1}"];
    for pattern in safe {
        assert!(
            validator.analyze(pattern).facts.nested_quantifiers.is_empty(),
            "optional or exact-one outer quantifier must not be reported: {pattern}"
        );
    }

    let risky = ["(a+)+", "(a+)+?", "(a+){2}", "(a+){2,5}", "(a+){2,}"];
    for pattern in risky {
        let analysis = validator.analyze(pattern);
        assert_eq!(
            analysis.facts.nested_quantifiers.len(),
            1,
            "repeating outer quantifier should retain one advisory: {pattern}"
        );
    }
    Ok(())
}

#[test]
fn possessive_quantifiers_and_atomic_groups_block_the_nested_risk_path()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    for pattern in ["(a++)+", "(?>a+)+", "(a+){2}+"] {
        assert!(
            validator.analyze(pattern).facts.nested_quantifiers.is_empty(),
            "possessive or atomic protection should suppress the advisory: {pattern}"
        );
    }
    Ok(())
}

#[test]
fn excluded_regions_use_one_authority_for_embedded_code_and_nested_risk()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r"\Q(?{ literal })(a+)+\E[(?{ x })(a+)+](a+)+(?{ live })";
    let analysis = RegexValidator::new().analyze(pattern);

    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(analysis.facts.embedded_code.len(), 1);
    assert_eq!(
        analysis.facts.embedded_code[0].range.start,
        pattern.rfind("(?{").ok_or("missing live embedded-code opener")?
    );
    Ok(())
}

#[test]
fn policy_limits_consume_the_same_group_and_unicode_events()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 1,
        max_unicode_properties: 1,
        max_branch_reset_branches: 1,
    });
    let pattern = r"(?<=(?<=x))(?|a|b)\p{L}\p{N}";
    let analysis = validator.analyze(pattern);
    let codes = analysis
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&RegexDiagnosticCode::LookbehindNestingLimit));
    assert!(codes.contains(&RegexDiagnosticCode::BranchResetBranchLimit));
    assert!(codes.contains(&RegexDiagnosticCode::UnicodePropertyLimit));
    assert!(analysis.completeness.is_policy_limited());
    Ok(())
}

#[test]
fn malformed_structure_is_retained_without_fabricated_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexValidator::new().analyze("[unterminated");

    assert!(analysis.malformed);
    assert!(!analysis.is_exhausted());
    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.facts.embedded_code.is_empty());
    assert!(analysis.facts.nested_quantifiers.is_empty());
    Ok(())
}
