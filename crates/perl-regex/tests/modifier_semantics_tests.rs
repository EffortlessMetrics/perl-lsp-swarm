use perl_regex::{RegexAnalyzer, validator::RegexDiagnosticCode};
use perl_regex::analyzer::{
    CaptureMode, CharacterSetMode, ExtendedMode, FeatureState, ModifierRequirementKind,
    ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator, RequirementDisposition,
};

fn sequence(raw: &str, start: usize) -> Result<ModifierSequence, Box<dyn std::error::Error>> {
    ModifierSequence::new(raw, start)
        .ok_or_else(|| format!("modifier sequence offset overflow for {raw:?}").into())
}

fn profile(minor: u16, enhanced_xx: FeatureState) -> RegexLanguageProfile {
    RegexLanguageProfile::new(Some(PerlVersion::new(5, minor)), enhanced_xx)
}

#[test]
fn raw_order_repetition_and_source_ranges_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("ixxaiiee", 20)?,
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.sequence.raw, "ixxaiiee");
    assert_eq!(analysis.sequence.range.start, 20);
    assert_eq!(analysis.sequence.range.end, 28);
    assert_eq!(
        analysis.tokens.iter().map(|token| token.value).collect::<String>(),
        "ixxaiiee"
    );
    assert_eq!(analysis.tokens[0].range.start, 20);
    assert_eq!(analysis.tokens[2].range.start, 22);
    assert_eq!(analysis.tokens[7].range.end, 28);
    assert!(analysis.diagnostics.is_empty());

    assert!(analysis.effective.case_insensitive);
    assert_eq!(
        analysis.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Enabled }
    );
    assert_eq!(analysis.effective.character_set, CharacterSetMode::Ascii);
    assert_eq!(analysis.effective.substitution_evaluation_depth, 2);
    Ok(())
}

#[test]
fn x_and_xx_have_distinct_versioned_effective_states()
-> Result<(), Box<dyn std::error::Error>> {
    let x = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("x", 0)?,
        profile(24, FeatureState::Disabled),
    );
    assert_eq!(x.effective.extended, ExtendedMode::Extended);
    assert!(x.diagnostics.is_empty());

    let xx_too_old = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 10)?,
        profile(24, FeatureState::Disabled),
    );
    assert_eq!(
        xx_too_old.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Disabled }
    );
    assert_eq!(xx_too_old.diagnostics.len(), 1);
    assert_eq!(
        xx_too_old.diagnostics[0].code,
        RegexDiagnosticCode::ModifierRequiresPerlVersion
    );
    assert_eq!(xx_too_old.diagnostics[0].range.start, 11);

    let xx = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 0)?,
        profile(26, FeatureState::Disabled),
    );
    assert_eq!(
        xx.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Disabled }
    );
    assert!(xx.diagnostics.is_empty());

    let enhanced = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 0)?,
        profile(44, FeatureState::Enabled),
    );
    assert_eq!(
        enhanced.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Enabled }
    );
    assert!(enhanced.requirements.iter().any(|requirement| {
        matches!(requirement.kind, ModifierRequirementKind::Feature("enhanced_xx"))
            && requirement.disposition == RequirementDisposition::Satisfied
    }));
    Ok(())
}

#[test]
fn enhanced_xx_cannot_be_claimed_before_perl_544()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::QuoteRegex,
        sequence("xx", 30)?,
        profile(42, FeatureState::Enabled),
    );

    assert_eq!(
        analysis.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Disabled }
    );
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == RegexDiagnosticCode::ModifierRequiresFeature
            && diagnostic.range.start == 31
    }));
    assert!(analysis.requirements.iter().any(|requirement| {
        matches!(requirement.kind, ModifierRequirementKind::Feature("enhanced_xx"))
            && requirement.disposition == RequirementDisposition::Unsatisfied
    }));
    Ok(())
}

#[test]
fn unknown_profile_preserves_unknown_requirements_without_false_rejection()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xxan", 5)?,
        RegexLanguageProfile::unknown(),
    );

    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Unknown }
    );
    assert_eq!(analysis.effective.character_set, CharacterSetMode::Ascii);
    assert_eq!(analysis.effective.captures, CaptureMode::NonCapturingByDefault);
    assert!(analysis
        .requirements
        .iter()
        .any(|requirement| requirement.disposition == RequirementDisposition::Unknown));
    Ok(())
}

#[test]
fn a_aa_and_character_set_conflicts_are_not_collapsed()
-> Result<(), Box<dyn std::error::Error>> {
    let a = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("a", 0)?,
        profile(14, FeatureState::Disabled),
    );
    assert_eq!(a.effective.character_set, CharacterSetMode::Ascii);

    let aa = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("aa", 0)?,
        profile(14, FeatureState::Disabled),
    );
    assert_eq!(aa.effective.character_set, CharacterSetMode::AsciiRestricted);

    let conflict = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("ad", 50)?,
        profile(14, FeatureState::Disabled),
    );
    assert_eq!(conflict.effective.character_set, CharacterSetMode::Conflict);
    assert!(conflict.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == RegexDiagnosticCode::ConflictingCharacterSetModifiers
            && diagnostic.range.start == 51
    }));
    Ok(())
}

#[test]
fn n_and_post_514_modifiers_are_version_qualified()
-> Result<(), Box<dyn std::error::Error>> {
    let old_n = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("n", 4)?,
        profile(20, FeatureState::Disabled),
    );
    assert_eq!(old_n.effective.captures, CaptureMode::NonCapturingByDefault);
    assert_eq!(old_n.diagnostics[0].code, RegexDiagnosticCode::ModifierRequiresPerlVersion);

    let current_n = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("n", 4)?,
        profile(22, FeatureState::Disabled),
    );
    assert!(current_n.diagnostics.is_empty());

    let old_substitution = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("ar", 8)?,
        profile(12, FeatureState::Disabled),
    );
    assert_eq!(
        old_substitution
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::ModifierRequiresPerlVersion
            })
            .count(),
        2
    );
    Ok(())
}

#[test]
fn e_ee_and_r_remain_substitution_specific()
-> Result<(), Box<dyn std::error::Error>> {
    let substitution = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("eer", 0)?,
        profile(14, FeatureState::Disabled),
    );
    assert_eq!(substitution.effective.substitution_evaluation_depth, 2);
    assert!(substitution.effective.non_destructive);
    assert!(substitution.diagnostics.is_empty());

    let matching = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("eer", 10)?,
        profile(44, FeatureState::Disabled),
    );
    assert_eq!(matching.effective.substitution_evaluation_depth, 0);
    assert!(!matching.effective.non_destructive);
    assert_eq!(
        matching
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::ModifierNotAllowedForOperator
            })
            .count(),
        3
    );
    Ok(())
}

#[test]
fn c_d_s_and_r_have_operator_specific_meanings()
-> Result<(), Box<dyn std::error::Error>> {
    let matching = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("gc", 0)?,
        profile(44, FeatureState::Disabled),
    );
    assert!(matching.effective.global);
    assert!(matching.effective.keep_match_position);
    assert!(!matching.effective.transliteration.complement);

    let substitution = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("gc", 0)?,
        profile(44, FeatureState::Disabled),
    );
    assert!(substitution.effective.global);
    assert!(substitution.effective.keep_match_position);
    assert!(substitution.diagnostics.is_empty());

    let transliteration = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Transliteration,
        sequence("cdsr", 0)?,
        profile(14, FeatureState::Disabled),
    );
    assert!(transliteration.effective.transliteration.complement);
    assert!(transliteration.effective.transliteration.delete);
    assert!(transliteration.effective.transliteration.squash);
    assert!(transliteration.effective.transliteration.non_destructive);
    assert!(!transliteration.effective.single_line);
    assert_eq!(transliteration.effective.character_set, CharacterSetMode::Default);
    assert!(transliteration.diagnostics.is_empty());
    Ok(())
}

#[test]
fn unknown_modifiers_keep_exact_identity_and_range()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("iz", 100)?,
        profile(44, FeatureState::Disabled),
    );

    assert!(analysis.effective.case_insensitive);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, RegexDiagnosticCode::UnknownModifier);
    assert_eq!(analysis.diagnostics[0].code.as_str(), "unknown_modifier");
    assert_eq!(analysis.diagnostics[0].range.start, 101);
    assert_eq!(analysis.diagnostics[0].range.end, 102);
    Ok(())
}

#[test]
fn sequence_constructor_refuses_offset_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(ModifierSequence::new("xx", usize::MAX).is_none());
    Ok(())
}
