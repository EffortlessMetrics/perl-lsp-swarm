use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    CaptureLanguageProfile, EffectiveModifiers, FeatureState, PatternBoundaryKind,
    PatternControlAnalysis, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlResolution, PatternControlUnresolvedReason, PerlVersion, RegexLanguageProfile,
};

fn profile() -> CaptureLanguageProfile {
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Disabled),
        FeatureState::Enabled,
    )
}

fn analyze(pattern: &str) -> PatternControlAnalysis {
    RegexAnalyzer::analyze_pattern_controls(pattern, 0, EffectiveModifiers::default(), profile())
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
    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PatternControlDiagnosticCode::InvalidReference
        })
    );
}

#[test]
fn positive_relative_g_reference_is_rejected() {
    let analysis = analyze(r"(a)\g{+1}(b)");

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "unsupported");
    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PatternControlDiagnosticCode::InvalidReference
        })
    );
}

#[test]
fn multi_digit_plain_escape_only_uses_captures_already_opened() {
    let analysis = analyze(r"\10(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)");

    assert_eq!(analysis.facts.len(), 1);
    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::AmbiguousNumericEscape
        )
    ));
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn single_digit_plain_escape_remains_a_backreference() {
    let analysis = analyze(r"(a)\1");

    assert_eq!(analysis.facts.len(), 1);
    assert!(matches!(
        &analysis.facts[0].resolution,
        PatternControlResolution::Resolved { targets }
            if targets.len() == 1 && targets[0].index() == 0
    ));
}

#[test]
fn invalid_reference_operand_reports_an_unsupported_effect() {
    let analysis = analyze(r"(a)\g{+1}(b)");

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "unsupported");
    // The kind and the effect have to agree: an operand this spelling cannot carry never
    // reads a capture, so it must not be published as a capture read.
    assert_eq!(analysis.facts[0].effect, PatternControlEffect::Unsupported);
}

#[test]
fn interpolation_inside_a_quoted_literal_is_a_dynamic_boundary() {
    let analysis = analyze(r"\Q$runtime\E(?<x>b)\1");

    // `\Q...\E` quotes metacharacters but Perl still interpolates inside it, so the run
    // supplies pattern text at runtime and the analysis must not claim completeness.
    assert!(analysis.status.dynamic_pattern);
    assert!(!analysis.status.is_complete());
    assert!(analysis.boundaries.iter().any(|boundary| {
        boundary.kind == PatternBoundaryKind::SourceInterpolation
            && (boundary.range.start, boundary.range.end) == (2, 10)
    }));

    let backreference = analysis
        .facts
        .iter()
        .find(|fact| fact.kind.as_str() == "numeric_backreference")
        .expect("numeric backreference fact");
    assert!(matches!(&backreference.resolution, PatternControlResolution::DynamicUnknown { .. }));
}

#[test]
fn a_quoted_literal_without_interpolation_stays_complete() {
    let analysis = analyze(r"\Qliteral\E(?<x>b)\1");

    // The discriminating half of the case above: quoting alone is static, so a quoted run
    // must not be turned into a blanket dynamic boundary.
    assert!(!analysis.status.dynamic_pattern);
    assert!(analysis.status.is_complete());
    assert_eq!(analysis.facts.len(), 1);
    assert!(matches!(
        &analysis.facts[0].resolution,
        PatternControlResolution::Resolved { targets }
            if targets.len() == 1 && targets[0].index() == 0
    ));
}

#[test]
fn escaped_sigil_inside_a_quoted_literal_is_not_interpolation() {
    let analysis = analyze(r"\Q\$runtime\E(?<x>b)\1");

    assert!(!analysis.status.dynamic_pattern);
    assert!(analysis.status.is_complete());
}

#[test]
fn assertion_conditional_predicates_are_unsupported_not_malformed() {
    for pattern in [r"(?(?=x)yes|no)", r"(?(?<=x)yes|no)"] {
        let analysis = analyze(pattern);

        // Perl accepts an assertion predicate, so it is unmodelled input rather than a
        // spelling error, and the two must stay distinguishable to consumers.
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == PatternControlDiagnosticCode::UnsupportedControl
            }),
            "{pattern} should report an unsupported control"
        );
        assert!(
            !analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == PatternControlDiagnosticCode::InvalidReference
            }),
            "{pattern} must not be reported as an invalid reference"
        );
    }
}

#[test]
fn a_malformed_conditional_predicate_is_still_an_invalid_reference() {
    let analysis = analyze(r"(?(?#x)yes|no)");

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PatternControlDiagnosticCode::InvalidReference
        })
    );
}

#[test]
fn later_structural_uncertainty_fails_a_forward_relative_call_closed() {
    let analysis = analyze(r"(a)(?+1)(*ACCEPT)");

    // A later star control leaves forward numbering unknown, so the missing target is not
    // evidence of a missing capture and must not become a hard reference error.
    let call = analysis
        .facts
        .iter()
        .find(|fact| fact.kind.as_str() == "relative_subpattern_call")
        .expect("relative subpattern call fact");
    assert!(matches!(&call.resolution, PatternControlResolution::StructuralUnknown { .. }));
    assert!(!analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::UnresolvedReference
    }));
}

#[test]
fn a_genuinely_missing_forward_relative_target_stays_unresolved() {
    let analysis = analyze(r"(a)(?+1)");

    // The discriminating half: with no structural uncertainty the reference really is
    // missing, and the fail-closed rule above must not swallow that.
    assert_eq!(analysis.facts.len(), 1);
    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::Unresolved(PatternControlUnresolvedReason::MissingCaptureNumber)
    ));
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::UnresolvedReference
    }));
}

#[test]
fn a_numeric_conditional_predicate_is_not_numbered_as_a_capture() {
    let analysis = analyze(r"(?(1)yes|no)(a)\1");

    // End-to-end cover for the predicate-numbering rule: `(a)` is capture 1, so `\1`
    // resolves to it rather than to a phantom declaration invented for `(1)`.
    assert_eq!(analysis.captures.declarations.len(), 1);
    assert_eq!(analysis.facts.len(), 2);
    assert_eq!(analysis.facts[0].kind.as_str(), "capture_conditional_number");
    assert_eq!(analysis.facts[1].kind.as_str(), "numeric_backreference");
    assert!(matches!(
        &analysis.facts[1].resolution,
        PatternControlResolution::Resolved { targets }
            if targets.len() == 1 && targets[0].index() == 0
    ));
    assert!(analysis.status.is_complete());
}
