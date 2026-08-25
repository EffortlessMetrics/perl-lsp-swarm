use perl_regex::RegexAnalyzer;
use perl_regex::analyzer::{
    CaptureLanguageProfile, EffectiveModifiers, FeatureState, PatternBoundaryKind,
    PatternControlAnalysis, PatternControlDiagnosticCode, PatternControlResolution,
    PatternControlUnresolvedReason, PatternExtendedMode, PerlVersion, RegexLanguageProfile,
};
use perl_regex::validator::RegexDiagnosticClass;

fn profile(minor: u16) -> CaptureLanguageProfile {
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(Some(PerlVersion::new(5, minor)), FeatureState::Disabled),
        FeatureState::Enabled,
    )
}

fn analyze(pattern: &str, source_start: usize) -> PatternControlAnalysis {
    RegexAnalyzer::analyze_pattern_controls(
        pattern,
        source_start,
        EffectiveModifiers::default(),
        profile(44),
    )
}

fn resolved_target_indexes(resolution: &PatternControlResolution) -> Vec<usize> {
    match resolution {
        PatternControlResolution::Resolved { targets } => {
            targets.iter().map(|target| target.index()).collect()
        }
        _ => Vec::new(),
    }
}

#[test]
fn keep_anchor_has_exact_body_and_original_source_ranges() -> Result<(), Box<dyn std::error::Error>>
{
    let analysis = analyze(r"(?<x>foo)\Kbar", 40);

    assert_eq!(analysis.facts.len(), 1);
    let fact = &analysis.facts[0];
    assert_eq!(fact.id.index(), 0);
    assert_eq!(fact.kind.as_str(), "keep_anchor");
    assert_eq!((fact.range.start, fact.range.end), (9, 11));
    let source = fact.source_range.ok_or("missing source range")?;
    assert_eq!((source.start, source.end), (49, 51));
    assert!(fact.resolution.is_exact());
    assert!(analysis.status.is_complete());
    Ok(())
}

#[test]
fn named_numeric_and_python_backreferences_share_capture_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<x>a)(b)\k<x>\2(?P=x)", 0);

    assert_eq!(analysis.captures.declarations.len(), 2);
    assert_eq!(analysis.facts.len(), 3);
    assert_eq!(analysis.facts[0].kind.as_str(), "named_backreference");
    assert_eq!(analysis.facts[1].kind.as_str(), "numeric_backreference");
    assert_eq!(analysis.facts[2].kind.as_str(), "named_backreference");
    assert_eq!(resolved_target_indexes(&analysis.facts[0].resolution), vec![0]);
    assert_eq!(resolved_target_indexes(&analysis.facts[1].resolution), vec![1]);
    assert_eq!(resolved_target_indexes(&analysis.facts[2].resolution), vec![0]);
    assert!(analysis.diagnostics.is_empty());
    Ok(())
}

#[test]
fn duplicate_names_resolve_to_every_source_backed_declaration()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<x>a)(?<x>b)\k<x>", 0);

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(resolved_target_indexes(&analysis.facts[0].resolution), vec![0, 1]);
    Ok(())
}

#[test]
fn branch_reset_number_references_preserve_all_alias_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?|(a)|(b))\1", 0);

    assert_eq!(analysis.captures.declarations.len(), 2);
    assert_eq!(analysis.captures.declarations[0].number, Some(1));
    assert_eq!(analysis.captures.declarations[1].number, Some(1));
    assert_eq!(resolved_target_indexes(&analysis.facts[0].resolution), vec![0, 1]);
    Ok(())
}

#[test]
fn forward_and_relative_subpattern_references_resolve_without_text_search()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?+1)(a)\g{-1}", 0);

    assert_eq!(analysis.facts.len(), 2);
    assert_eq!(analysis.facts[0].kind.as_str(), "relative_subpattern_call");
    assert_eq!(analysis.facts[1].kind.as_str(), "relative_backreference");
    assert_eq!(resolved_target_indexes(&analysis.facts[0].resolution), vec![0]);
    assert_eq!(resolved_target_indexes(&analysis.facts[1].resolution), vec![0]);
    Ok(())
}

#[test]
fn recursion_subpattern_calls_and_conditionals_are_distinct_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<x>a)(?R)(?&x)(?(<x>)yes|no)(?(R)recur|plain)", 0);

    assert_eq!(
        analysis.facts.iter().map(|fact| fact.kind.as_str()).collect::<Vec<_>>(),
        vec![
            "whole_pattern_recursion",
            "named_subpattern_call",
            "capture_conditional_name",
            "recursion_conditional",
        ]
    );
    assert_eq!(resolved_target_indexes(&analysis.facts[1].resolution), vec![0]);
    assert_eq!(resolved_target_indexes(&analysis.facts[2].resolution), vec![0]);
    assert!(analysis.facts[0].resolution.is_exact());
    assert!(analysis.facts[3].resolution.is_exact());
    Ok(())
}

#[test]
fn interpolation_preserves_earlier_exact_facts_and_qualifies_later_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<before>a)\k<before>$runtime(?<after>b)\k<after>", 0);

    assert_eq!(analysis.facts.len(), 3);
    assert!(matches!(analysis.facts[0].resolution, PatternControlResolution::Resolved { .. }));
    assert_eq!(analysis.facts[1].kind.as_str(), "source_interpolation");
    assert!(matches!(
        analysis.facts[2].resolution,
        PatternControlResolution::DynamicUnknown { .. }
    ));
    assert!(analysis.status.dynamic_pattern);
    assert!(
        analysis
            .boundaries
            .iter()
            .any(|boundary| { boundary.kind == PatternBoundaryKind::SourceInterpolation })
    );
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::DynamicPatternBoundary
            && diagnostic.class == RegexDiagnosticClass::DynamicBoundary
    }));
    Ok(())
}

#[test]
fn deferred_runtime_regex_text_is_a_typed_completeness_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<x>a)(??{ build() })\k<x>", 0);

    assert_eq!(analysis.facts[0].kind.as_str(), "deferred_runtime_pattern");
    assert!(matches!(
        analysis.facts[1].resolution,
        PatternControlResolution::DynamicUnknown { .. }
    ));
    assert!(analysis.status.dynamic_pattern);
    assert!(
        analysis
            .boundaries
            .iter()
            .any(|boundary| { boundary.kind == PatternBoundaryKind::RuntimePattern })
    );
    Ok(())
}

#[test]
fn quoted_classes_and_comments_do_not_create_reference_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"\Q\k<x>(?R)\E[\1](?#(?&x)(?<x>a)\k<x>", 0);

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "named_backreference");
    assert_eq!(resolved_target_indexes(&analysis.facts[0].resolution), vec![0]);
    Ok(())
}

#[test]
fn complete_missing_reference_is_a_syntax_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?<x>a)\k<missing>", 0);

    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::Unresolved(PatternControlUnresolvedReason::MissingCaptureName)
    ));
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, PatternControlDiagnosticCode::UnresolvedReference);
    assert_eq!(analysis.diagnostics[0].class, RegexDiagnosticClass::Syntax);
    Ok(())
}

#[test]
fn incompatible_capture_profile_is_not_reported_as_a_missing_name()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexAnalyzer::analyze_pattern_controls(
        r"(?<x>a)\k<x>",
        0,
        EffectiveModifiers::default(),
        profile(8),
    );

    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::Unresolved(PatternControlUnresolvedReason::ProfileIncompatible)
    ));
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::ProfileIncompatibleReference
    }));
    Ok(())
}

#[test]
fn unsupported_star_controls_fail_closed_without_renumbering_guesses()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(*ACCEPT)(?<x>a)\1", 0);

    assert_eq!(analysis.facts[0].kind.as_str(), "unsupported");
    assert!(matches!(
        analysis.facts[1].resolution,
        PatternControlResolution::StructuralUnknown { .. }
    ));
    assert!(analysis.status.unsupported);
    assert!(analysis.status.structural_uncertainty);
    assert!(
        analysis
            .boundaries
            .iter()
            .any(|boundary| { boundary.kind == PatternBoundaryKind::UnsupportedControl })
    );
    Ok(())
}

#[test]
fn optimistic_embedded_code_is_execution_not_runtime_pattern_text()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(*{ side_effect() })(?<x>a)\k<x>", 0);

    assert_eq!(analysis.facts[0].kind.as_str(), "optimistic_embedded_code");
    assert!(analysis.status.dynamic_execution);
    assert!(!analysis.status.dynamic_pattern);
    assert!(analysis.status.structural_uncertainty);
    assert!(
        analysis
            .boundaries
            .iter()
            .any(|boundary| { boundary.kind == PatternBoundaryKind::EmbeddedCodeExecution })
    );
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PatternControlDiagnosticCode::EmbeddedCodeBoundary
    }));
    Ok(())
}

#[test]
fn local_modifier_scope_is_retained_on_each_fact() -> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"(?x:\K)(?-x:\K)", 0);

    assert_eq!(analysis.facts.len(), 2);
    assert_eq!(analysis.facts[0].local_mode.extended, PatternExtendedMode::Extended);
    assert_eq!(analysis.facts[1].local_mode.extended, PatternExtendedMode::Off);
    Ok(())
}

#[test]
fn malformed_reference_spelling_is_retained_and_diagnosed() -> Result<(), Box<dyn std::error::Error>>
{
    let analysis = analyze(r"\k<unclosed", 0);

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "unsupported");
    assert_eq!(analysis.diagnostics[0].code, PatternControlDiagnosticCode::InvalidReference);
    assert!(analysis.status.unsupported);
    Ok(())
}

#[test]
fn source_offset_overflow_fails_mapping_without_fabricating_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze(r"\K", usize::MAX);

    assert_eq!(analysis.facts.len(), 1);
    assert!(analysis.facts[0].source_range.is_none());
    assert!(!analysis.status.source_mapping_complete);
    assert!(!analysis.status.is_complete());
    Ok(())
}

#[test]
fn unmatched_paren_after_a_comment_keeps_numbering_structurally_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    // A `(?#...)` comment ends at the first `)`, so the trailing `)` here is unmatched
    // and the pattern is not valid Perl. The reference is still recognized, but
    // numbering must fail closed rather than report an exact target.
    let analysis = analyze(r"\Q\k<x>(?R)\E[\1](?#(?&x))(?<x>a)\k<x>", 0);

    assert_eq!(analysis.facts.len(), 1);
    assert_eq!(analysis.facts[0].kind.as_str(), "named_backreference");
    assert!(matches!(
        analysis.facts[0].resolution,
        PatternControlResolution::StructuralUnknown { .. }
    ));
    Ok(())
}
