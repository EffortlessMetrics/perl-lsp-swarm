use perl_regex::RegexValidator;
use perl_regex::validator::{
    RegexAnalysisCompleteness, RegexDiagnosticCode, RegexDynamicRegionKind, RegexValidationConfig,
};

#[test]
fn immediate_code_uses_the_full_balanced_span_and_masks_inner_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r#"(?{ my $x = { nested => "}" }; "(a+)+"; \p{L}\p{N}; }) (b+)+"#;
    let live_start = pattern.find(" (b+)+").ok_or("missing live regex suffix")? + 1;

    let analysis = validator.analyze(pattern);

    assert_eq!(analysis.facts.embedded_code.len(), 1);
    let embedded = &analysis.facts.embedded_code[0];
    assert_eq!(embedded.range.start, 0);
    assert_eq!(embedded.range.end, live_start - 1);
    assert_eq!(analysis.facts.dynamic_regions.len(), 1);
    assert_eq!(
        analysis.facts.dynamic_regions[0].kind,
        RegexDynamicRegionKind::EmbeddedCodeImmediate
    );
    assert_eq!(analysis.facts.dynamic_regions[0].range, embedded.range);

    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert!(analysis.facts.nested_quantifiers[0].start >= live_start);
    assert!(
        analysis
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != RegexDiagnosticCode::UnicodePropertyLimit)
    );
    assert_eq!(
        analysis.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>(),
        vec![RegexDiagnosticCode::EmbeddedCodeImmediate, RegexDiagnosticCode::NestedQuantifierRisk,]
    );
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::Dynamic);
    Ok(())
}

#[test]
fn deferred_code_uses_one_full_region_and_does_not_double_count_nested_text()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r#"prefix(??{ build({ key => "}" }); "(?{ nested })" })suffix"#;
    let analysis = RegexValidator::new().analyze(pattern);

    assert_eq!(analysis.facts.embedded_code.len(), 1);
    assert_eq!(analysis.facts.dynamic_regions.len(), 1);
    assert_eq!(
        analysis.facts.dynamic_regions[0].kind,
        RegexDynamicRegionKind::EmbeddedCodeDeferred
    );
    assert_eq!(
        pattern.get(
            analysis.facts.dynamic_regions[0].range.start
                ..analysis.facts.dynamic_regions[0].range.end
        ),
        Some(r#"(??{ build({ key => "}" }); "(?{ nested })" })"#)
    );
    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| { diagnostic.code == RegexDiagnosticCode::EmbeddedCodeDeferred })
            .count(),
        1
    );
    Ok(())
}

#[test]
fn interpolation_is_a_typed_dynamic_boundary_without_becoming_a_syntax_error()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r"before$runtime${expr{nested}}@valuesafter";
    let analysis = RegexValidator::new().analyze(pattern);

    assert_eq!(analysis.facts.dynamic_regions.len(), 3);
    assert!(
        analysis
            .facts
            .dynamic_regions
            .iter()
            .all(|fact| fact.kind == RegexDynamicRegionKind::Interpolation)
    );
    assert_eq!(
        pattern.get(
            analysis.facts.dynamic_regions[0].range.start
                ..analysis.facts.dynamic_regions[0].range.end
        ),
        Some("$runtime")
    );
    assert_eq!(
        pattern.get(
            analysis.facts.dynamic_regions[1].range.start
                ..analysis.facts.dynamic_regions[1].range.end
        ),
        Some("${expr{nested}}")
    );
    assert_eq!(
        pattern.get(
            analysis.facts.dynamic_regions[2].range.start
                ..analysis.facts.dynamic_regions[2].range.end
        ),
        Some("@valuesafter")
    );
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::Dynamic);
    Ok(())
}

#[test]
fn interpolation_looking_text_in_excluded_regions_is_not_dynamic()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r"\Q$quoted @quoted\E[$class](?# $comment @comment )\$escaped\@escaped";
    let analysis = RegexValidator::new().analyze(pattern);

    assert!(analysis.facts.dynamic_regions.is_empty());
    assert!(analysis.completeness.is_complete());
    assert!(analysis.diagnostics.is_empty());
    Ok(())
}

#[test]
fn compatibility_finder_projects_the_start_of_the_full_dynamic_region()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r#"xx(?{ { nested => 1 } })yy"#;
    let validator = RegexValidator::new();
    let analysis = validator.analyze(pattern);
    let fact = analysis.facts.embedded_code.first().ok_or("missing embedded code")?;
    assert_eq!(pattern.get(fact.range.start..fact.range.end), Some("(?{ { nested => 1 } })"));

    let compatibility =
        validator.find_code_execution(pattern, 100).ok_or("missing compatibility finding")?;
    assert_eq!(compatibility.offset, 102);
    Ok(())
}

#[test]
fn complexity_offsets_anchor_following_groups_after_dynamic_regions()
-> Result<(), Box<dyn std::error::Error>> {
    let pattern = r"(?{ code })(?<=a)(?|b|c)";
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 0,
        max_unicode_properties: 10,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze(pattern);
    let lookbehind_start = pattern.find("(?<=").ok_or("missing lookbehind")?;
    let branch_reset_start = pattern.find("(?|").ok_or("missing branch reset")?;

    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| { diagnostic.code == RegexDiagnosticCode::LookbehindNestingLimit })
            .map(|diagnostic| diagnostic.range.start)
            .collect::<Vec<_>>(),
        vec![lookbehind_start]
    );
    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::BranchResetNestingLimit
            })
            .map(|diagnostic| diagnostic.range.start)
            .collect::<Vec<_>>(),
        vec![branch_reset_start]
    );
    Ok(())
}
