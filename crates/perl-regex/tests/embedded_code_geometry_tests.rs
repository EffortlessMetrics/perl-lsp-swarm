#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_regex::validator::{RegexDiagnosticCode, RegexDynamicRegionKind};
use perl_regex::RegexValidator;

#[test]
fn embedded_code_facts_cover_full_constructs_while_diagnostics_anchor_openers()
-> Result<(), Box<dyn std::error::Error>> {
    let immediate = "(?{ run() })";
    let deferred = "(??{ later() })";
    let pattern = format!("é{immediate}中{deferred}");
    let analysis = RegexValidator::new().analyze(&pattern);

    assert_eq!(analysis.facts.embedded_code.len(), 2);
    assert_eq!(analysis.facts.dynamic_regions.len(), 2);

    let expected = [
        (
            immediate,
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
        ),
        (
            deferred,
            RegexDynamicRegionKind::EmbeddedCodeDeferred,
            RegexDiagnosticCode::EmbeddedCodeDeferred,
            "(??{",
        ),
    ];

    for (index, (construct, dynamic_kind, diagnostic_code, opener)) in
        expected.into_iter().enumerate()
    {
        let fact_range = analysis.facts.embedded_code[index].range;
        assert_eq!(pattern.get(fact_range.start..fact_range.end), Some(construct));
        assert!(pattern.is_char_boundary(fact_range.start));
        assert!(pattern.is_char_boundary(fact_range.end));

        let dynamic_region = &analysis.facts.dynamic_regions[index];
        assert_eq!(dynamic_region.kind, dynamic_kind);
        assert_eq!(dynamic_region.range, fact_range);

        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == diagnostic_code)
            .ok_or("missing embedded-code diagnostic")?;
        assert_eq!(
            pattern.get(diagnostic.range.start..diagnostic.range.end),
            Some(opener)
        );
    }

    Ok(())
}

#[test]
fn embedded_code_bodies_are_excluded_from_structural_regex_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let hidden = r#"(?{ my $s = '(a+)+' })"#;
    let live = "(a+)+";

    let hidden_only = validator.analyze(hidden);
    assert!(hidden_only.facts.nested_quantifiers.is_empty());

    let live_only = validator.analyze(live);
    assert_eq!(live_only.facts.nested_quantifiers.len(), 1);

    let mixed = format!("{hidden}{live}");
    let mixed_analysis = validator.analyze(&mixed);
    assert_eq!(mixed_analysis.facts.embedded_code.len(), 1);
    assert_eq!(mixed_analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(
        mixed.get(
            mixed_analysis.facts.embedded_code[0].range.start
                ..mixed_analysis.facts.embedded_code[0].range.end
        ),
        Some(hidden)
    );
    assert_eq!(
        mixed_analysis.facts.nested_quantifiers[0].start,
        mixed.rfind('+').ok_or("missing live outer quantifier")?
    );

    Ok(())
}
