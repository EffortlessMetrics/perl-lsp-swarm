use perl_regex::analyzer::{
    CaptureMode, CharacterSetMode, ExtendedMode, FeatureState, ModifierRequirementKind,
    ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator, RequirementDisposition,
};
use perl_regex::{RegexAnalyzer, validator::RegexDiagnosticCode};

fn sequence(raw: &str, start: usize) -> Result<ModifierSequence, Box<dyn std::error::Error>> {
    ModifierSequence::new(raw, start)
        .ok_or_else(|| format!("modifier sequence offset overflow for {raw:?}").into())
}

fn profile(minor: u16, enhanced_xx: FeatureState) -> RegexLanguageProfile {
    RegexLanguageProfile::new(Some(PerlVersion::new(5, minor)), enhanced_xx)
}

#[test]
fn raw_order_repetition_and_source_ranges_are_preserved() -> Result<(), Box<dyn std::error::Error>>
{
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("ixxaiiee", 20)?,
        profile(44, FeatureState::Enabled),
    );

    assert_eq!(analysis.sequence.raw, "ixxaiiee");
    assert_eq!(analysis.sequence.range.start, 20);
    assert_eq!(analysis.sequence.range.end, 28);
    assert_eq!(analysis.tokens.iter().map(|token| token.value).collect::<String>(), "ixxaiiee");
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
fn x_and_xx_have_distinct_versioned_effective_states() -> Result<(), Box<dyn std::error::Error>> {
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
    // Pre-5.26 Perl cannot compile `/xx`, so the admitted behavior is plain
    // `/x`; reporting ExtraExtended here would let a consumer apply semantics
    // the selected Perl does not have.
    assert_eq!(xx_too_old.effective.extended, ExtendedMode::Extended);
    assert_eq!(xx_too_old.diagnostics.len(), 1);
    assert_eq!(xx_too_old.diagnostics[0].code, RegexDiagnosticCode::ModifierRequiresPerlVersion);
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
fn enhanced_xx_cannot_be_claimed_before_perl_544() -> Result<(), Box<dyn std::error::Error>> {
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
    assert!(
        analysis
            .requirements
            .iter()
            .any(|requirement| requirement.disposition == RequirementDisposition::Unknown)
    );
    Ok(())
}

#[test]
fn a_aa_and_character_set_conflicts_are_not_collapsed() -> Result<(), Box<dyn std::error::Error>> {
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
fn n_and_post_514_modifiers_are_version_qualified() -> Result<(), Box<dyn std::error::Error>> {
    let old_n = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("n", 4)?,
        profile(20, FeatureState::Disabled),
    );
    // The requirement fails, so the effect is withheld: `effective` stays at the
    // capturing default while the raw `n` remains in the preserved sequence.
    assert_eq!(old_n.effective.captures, CaptureMode::CapturingByDefault);
    assert_eq!(old_n.sequence.raw, "n");
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
fn e_ee_and_r_remain_substitution_specific() -> Result<(), Box<dyn std::error::Error>> {
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
fn c_d_s_and_r_have_operator_specific_meanings() -> Result<(), Box<dyn std::error::Error>> {
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
    // Perl warns that `/c` is meaningless in `s///`: it does not preserve a
    // substitution match position, so this must differ from the match case above.
    assert!(!substitution.effective.keep_match_position);
    assert!(
        substitution
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RegexDiagnosticCode::ModifierHasNoEffect)
    );

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
fn unknown_modifiers_keep_exact_identity_and_range() -> Result<(), Box<dyn std::error::Error>> {
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
fn sequence_constructor_refuses_offset_overflow() -> Result<(), Box<dyn std::error::Error>> {
    assert!(ModifierSequence::new("xx", usize::MAX).is_none());
    Ok(())
}

#[test]
fn repeated_character_set_modifiers_are_rejected_at_the_exact_token()
-> Result<(), Box<dyn std::error::Error>> {
    // Perl allows `/a` at most twice and `/d`, `/l`, `/u` only once.
    for (raw, offending) in [("aaa", 2usize), ("dd", 1), ("ll", 1), ("uu", 1)] {
        let analysis = RegexAnalyzer::analyze_modifiers(
            RegexOperator::Match,
            sequence(raw, 0)?,
            profile(44, FeatureState::Enabled),
        );
        let repeats = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::RepeatedCharacterSetModifier
            })
            .collect::<Vec<_>>();
        assert_eq!(repeats.len(), 1, "/{raw} must report exactly one repetition");
        assert_eq!(repeats[0].range.start, offending, "/{raw} must diagnose the offending token");
    }

    // Controls: the legal single and double forms stay clean, including the
    // separated double `a` in `/aia`.
    for raw in ["a", "aa", "aia", "d", "l", "u"] {
        let analysis = RegexAnalyzer::analyze_modifiers(
            RegexOperator::Match,
            sequence(raw, 0)?,
            profile(44, FeatureState::Enabled),
        );
        assert!(
            !analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::RepeatedCharacterSetModifier
            }),
            "/{raw} is legal and must not report repetition"
        );
    }
    assert_eq!(
        RegexAnalyzer::analyze_modifiers(
            RegexOperator::Match,
            sequence("aia", 0)?,
            profile(44, FeatureState::Enabled),
        )
        .effective
        .character_set,
        CharacterSetMode::AsciiRestricted
    );
    Ok(())
}

#[test]
fn unsatisfied_version_requirements_withhold_the_effective_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    // A consumer reading `effective` must not be able to apply behavior the
    // selected Perl cannot compile.
    let n_on_520 = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("n", 0)?,
        profile(20, FeatureState::Disabled),
    );
    assert!(
        n_on_520
            .diagnostics
            .iter()
            .any(|d| d.code == RegexDiagnosticCode::ModifierRequiresPerlVersion)
    );
    assert_eq!(n_on_520.effective.captures, CaptureMode::CapturingByDefault);

    let a_on_512 = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("a", 0)?,
        profile(12, FeatureState::Disabled),
    );
    assert_eq!(a_on_512.effective.character_set, CharacterSetMode::Default);

    let r_on_512 = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("r", 0)?,
        profile(12, FeatureState::Disabled),
    );
    assert!(!r_on_512.effective.non_destructive);

    // Pre-5.26 the second `x` is not `/xx`; admitted behavior stays plain `/x`.
    let xx_on_520 = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 0)?,
        profile(20, FeatureState::Disabled),
    );
    assert_eq!(xx_on_520.effective.extended, ExtendedMode::Extended);

    // Control: on a satisfying profile the same forms do take effect.
    let modern = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("n", 0)?,
        profile(44, FeatureState::Enabled),
    );
    assert_eq!(modern.effective.captures, CaptureMode::NonCapturingByDefault);
    Ok(())
}

#[test]
fn unknown_perl_version_cannot_prove_enhanced_xx() -> Result<(), Box<dyn std::error::Error>> {
    // Version unknown but the feature pragma is enabled: the 5.44 boundary is
    // unestablished, so enhanced state must stay Unknown, not Enabled.
    let analysis = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 0)?,
        RegexLanguageProfile::new(None, FeatureState::Enabled),
    );
    assert_eq!(
        analysis.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Unknown },
        "an enabling pragma alone must not select newest semantics"
    );
    assert!(analysis.requirements.iter().any(|requirement| {
        matches!(requirement.kind, ModifierRequirementKind::Feature(_))
            && requirement.disposition == RequirementDisposition::Unknown
    }));

    // A known-disabled feature stays disabled even with an unknown version.
    let disabled = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("xx", 0)?,
        RegexLanguageProfile::new(None, FeatureState::Disabled),
    );
    assert_eq!(
        disabled.effective.extended,
        ExtendedMode::ExtraExtended { enhanced: FeatureState::Disabled }
    );
    Ok(())
}

#[test]
fn raw_sequences_round_trip_losslessly_across_operators_and_profiles()
-> Result<(), Box<dyn std::error::Error>> {
    // The headline claim is losslessness: whatever a diagnostic or derived mode
    // says, the exact raw spelling, order, repetition, and per-token source
    // ranges must survive for every operator and profile, including sequences
    // that are illegal, conflicting, version-gated, or unknown.
    let raws = [
        "", "i", "xx", "xxx", "aa", "aaa", "adlu", "dd", "gc", "cg", "ee", "eee", "r", "n",
        "ixxaiiee", "gcer", "cdsr", "iZq", "xXx",
    ];
    let operators = [
        RegexOperator::BareMatch,
        RegexOperator::Match,
        RegexOperator::QuoteRegex,
        RegexOperator::Substitution,
        RegexOperator::Transliteration,
        RegexOperator::TransliterationAlias,
    ];
    let profiles = [
        RegexLanguageProfile::unknown(),
        profile(12, FeatureState::Disabled),
        profile(26, FeatureState::Unknown),
        profile(44, FeatureState::Enabled),
        RegexLanguageProfile::new(None, FeatureState::Enabled),
    ];

    for raw in raws {
        for operator in operators {
            for language in profiles {
                let start = 7usize;
                let analysis =
                    RegexAnalyzer::analyze_modifiers(operator, sequence(raw, start)?, language);

                assert_eq!(analysis.sequence.raw, raw, "raw spelling for /{raw}");
                assert_eq!(
                    analysis.tokens.iter().map(|token| token.value).collect::<String>(),
                    raw,
                    "token order and repetition for /{raw}"
                );
                assert_eq!(
                    analysis.tokens.len(),
                    raw.chars().count(),
                    "no modifier may be normalized away for /{raw}"
                );
                assert_eq!(analysis.sequence.range.start, start);
                assert_eq!(analysis.sequence.range.end, start + raw.len());
                assert_eq!(analysis.operator, operator);

                // Token ranges stay contiguous and inside the sequence range.
                let mut cursor = start;
                for token in &analysis.tokens {
                    assert_eq!(token.range.start, cursor, "token start for /{raw}");
                    assert_eq!(token.range.end, cursor + token.value.len_utf8());
                    cursor = token.range.end;
                }
                assert_eq!(cursor, analysis.sequence.range.end);

                // Every diagnostic points inside the sequence it came from.
                for diagnostic in &analysis.diagnostics {
                    assert!(
                        diagnostic.range.start >= start && diagnostic.range.end <= cursor,
                        "diagnostic range escapes the sequence for /{raw}"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn substitution_c_without_g_is_diagnosed_and_does_not_preserve_position()
-> Result<(), Box<dyn std::error::Error>> {
    // Perl accepts `/c` on `s///` but emits a "meaningless use of /c" warning
    // because match-position preservation only applies to `m//gc`.  Static
    // analysis must mirror that: `s///c` must emit a `ModifierHasNoEffect`
    // diagnostic and must NOT set `keep_match_position`.
    //
    // This is the discriminating negative case: the existing suite already
    // covers `s///gc`, but the defect claim is that even `s///c` (without `/g`)
    // was reported as harmless.  Without a discriminating test a suite that only
    // asserts the well-formed `m//gc` spelling passes with the defect present.
    let s_c = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Substitution,
        sequence("c", 0)?,
        profile(44, FeatureState::Disabled),
    );
    assert!(!s_c.effective.keep_match_position, "s///c must not set keep_match_position");
    assert!(
        s_c.diagnostics.iter().any(|d| d.code == RegexDiagnosticCode::ModifierHasNoEffect),
        "s///c must emit ModifierHasNoEffect; got: {:?}",
        s_c.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
    );

    // Contrast: `m//gc` is the one valid form that preserves a match position.
    let m_gc = RegexAnalyzer::analyze_modifiers(
        RegexOperator::Match,
        sequence("gc", 0)?,
        profile(44, FeatureState::Disabled),
    );
    assert!(m_gc.effective.keep_match_position, "m//gc must set keep_match_position");
    assert!(
        !m_gc.diagnostics.iter().any(|d| d.code == RegexDiagnosticCode::ModifierHasNoEffect),
        "m//gc must not emit ModifierHasNoEffect"
    );
    Ok(())
}
