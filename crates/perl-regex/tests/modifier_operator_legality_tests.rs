use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    FeatureState, ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator,
};
use perl_regex::validator::RegexDiagnosticCode;

fn analyze(
    operator: RegexOperator,
    raw: &str,
) -> Result<perl_regex::analyzer::ModifierAnalysis, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new(raw, 0)
        .ok_or_else(|| format!("modifier range overflow for {raw:?}"))?;
    let profile = RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Enabled);
    Ok(RegexAnalyzer::analyze_modifiers(operator, sequence, profile))
}

#[test]
fn quote_regex_rejects_match_process_and_substitution_modifiers()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(RegexOperator::QuoteRegex, "gcer")?;

    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::ModifierNotAllowedForOperator
            })
            .count(),
        4
    );
    assert!(!analysis.effective.global);
    assert!(!analysis.effective.keep_match_position);
    assert_eq!(analysis.effective.substitution_evaluation_depth, 0);
    assert!(!analysis.effective.non_destructive);
    Ok(())
}

#[test]
fn substitution_c_is_inert_and_never_complement() -> Result<(), Box<dyn std::error::Error>> {
    // Perl accepts `s///gc` but warns that `/c` is meaningless there: unlike
    // `m//gc` it does not preserve a match position, and it is never the
    // transliteration complement.
    let analysis = analyze(RegexOperator::Substitution, "gc")?;

    assert!(analysis.effective.global);
    assert!(!analysis.effective.keep_match_position);
    assert!(!analysis.effective.transliteration.complement);
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RegexDiagnosticCode::ModifierHasNoEffect
                && diagnostic.range.start == 1),
        "substitution /c must be reported as having no effect, at the c token"
    );
    Ok(())
}

#[test]
fn match_c_keeps_position_only_with_g() -> Result<(), Box<dyn std::error::Error>> {
    let global = analyze(RegexOperator::Match, "gc")?;
    assert!(global.effective.keep_match_position);
    assert!(
        !global
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RegexDiagnosticCode::ModifierHasNoEffect)
    );

    // `/c` without `/g` is inert, and token order must not change that.
    for raw in ["c", "cg"] {
        let analysis = analyze(RegexOperator::Match, raw)?;
        assert_eq!(
            analysis.effective.keep_match_position,
            raw.contains('g'),
            "match /{raw} match-position state"
        );
    }
    Ok(())
}

#[test]
fn transliteration_rejects_regex_and_match_process_modifiers()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(RegexOperator::Transliteration, "gix")?;

    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::ModifierNotAllowedForOperator
            })
            .count(),
        3
    );
    assert!(!analysis.effective.global);
    assert!(!analysis.effective.case_insensitive);
    Ok(())
}
