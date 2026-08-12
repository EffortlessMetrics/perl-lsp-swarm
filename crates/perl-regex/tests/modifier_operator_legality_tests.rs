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
    let profile = RegexLanguageProfile::new(
        Some(PerlVersion::new(5, 44)),
        FeatureState::Enabled,
    );
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
fn substitution_accepts_c_as_match_position_state_not_complement()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(RegexOperator::Substitution, "gc")?;

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.effective.global);
    assert!(analysis.effective.keep_match_position);
    assert!(!analysis.effective.transliteration.complement);
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
