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
fn embedded_code_masks_complexity_but_preserves_outer_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r"(?{ \p{L}\p{N} })\p{L}\p{N}";
    let analysis = validator.analyze(pattern);

    assert_eq!(analysis.facts.dynamic_regions.len(), 1);
    let embedded_end = analysis.facts.dynamic_regions[0].range.end;
    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == RegexDiagnosticCode::UnicodePropertyLimit)
            .count(),
        1
    );
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == RegexDiagnosticCode::UnicodePropertyLimit
            && diagnostic.range.start >= embedded_end
    }));
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::DynamicAndPolicyLimited);
    Ok(())
}

#[test]
fn source_interpolation_is_not_yet_a_dynamic_region() -> Result<(), Box<dyn std::error::Error>> {
    // Follow-up seam: interpolation scanning was deferred from this hosted-ripr slice.
    let analysis = RegexValidator::new().analyze(r"before$runtime${expr}@valuesafter");
    assert!(analysis.facts.dynamic_regions.is_empty());
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::Complete);
    Ok(())
}

#[test]
fn complexity_offsets_anchor_following_groups_after_dynamic_regions()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 1,
        max_unicode_properties: 50,
        max_branch_reset_branches: 1,
    });
    let pattern = r"(?{ ignore })(?<=a(?<=b))";
    let analysis = validator.analyze(pattern);
    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == RegexDiagnosticCode::LookbehindNestingLimit
                && diagnostic.range.start
                    >= analysis.facts.dynamic_regions[0].range.end
        }),
        "{:?}",
        analysis.diagnostics
    );
    Ok(())
}
