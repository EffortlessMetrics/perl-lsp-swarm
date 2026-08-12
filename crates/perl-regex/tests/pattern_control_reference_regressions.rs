use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    CaptureLanguageProfile, EffectiveModifiers, FeatureState, PatternControlAnalysis,
    PatternControlDiagnosticCode, PatternControlResolution, PatternControlUnresolvedReason,
    PerlVersion, RegexLanguageProfile,
};

fn profile() -> CaptureLanguageProfile {
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(
            Some(PerlVersion::new(5, 44)),
            FeatureState::Disabled,
        ),
        FeatureState::Enabled,
    )
}

fn analyze(pattern: &str) -> PatternControlAnalysis {
    RegexAnalyzer::analyze_pattern_controls(
        pattern,
        0,
        EffectiveModifiers::default(),
        profile(),
    )
}

#[test]
fn g_braces_are_the_named_backreference_form() {
    let analysis = analyze(r"(?<name>a)\g{name}");

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "named_backreference");
    assert!(matches!(
        &analysis.facts[0].resolution,
        PatternControlResolution::Resolved { targets }
            if targets.len() == 1 && targets[0].index() == 0
    ));
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn g_angle_name_is_rejected_instead_of_becoming_a_perl_backreference() {
    let analysis = analyze(r"(?<name>a)\g<name>");

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "unsupported");
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::InvalidReference
    }));
}

#[test]
fn missing_plain_numeric_escape_remains_ambiguous_with_octal() {
    let analysis = analyze(r"\1");

    assert_eq!(analysis.facts.len(), 1);
    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::AmbiguousNumericEscape
        )
    ));
    assert!(analysis.diagnostics.is_empty());
}
