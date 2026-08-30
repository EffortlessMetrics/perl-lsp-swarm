#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_regex::RegexValidator;
use perl_regex::validator::{RegexDiagnosticCode, RegexDynamicRegionKind};

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
        assert_eq!(pattern.get(diagnostic.range.start..diagnostic.range.end), Some(opener));
    }

    Ok(())
}

#[test]
fn supported_embedded_code_forms_have_exact_construct_and_opener_spans()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            "prefix(?{ nested { braces } })suffix",
            "(?{ nested { braces } })",
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            false,
        ),
        (
            r#"before(??{ my $text = "quoted }"; })after"#,
            r#"(??{ my $text = "quoted }"; })"#,
            RegexDynamicRegionKind::EmbeddedCodeDeferred,
            RegexDiagnosticCode::EmbeddedCodeDeferred,
            "(??{",
            false,
        ),
        (
            r#"before(?{ my $text = "\"}"; })after"#,
            r#"(?{ my $text = "\"}"; })"#,
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            false,
        ),
        (
            "é(?{\r\n  body\r\n})中",
            "(?{\r\n  body\r\n})",
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            false,
        ),
        (
            "pre(?{ x中})post",
            "(?{ x中})",
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            false,
        ),
        (
            "pre(?{ missing paren }tail",
            "(?{ missing paren }",
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            true,
        ),
        (
            "pre(?{ missing close",
            "(?{ missing close",
            RegexDynamicRegionKind::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            "(?{",
            true,
        ),
    ];

    for (pattern, construct, dynamic_kind, diagnostic_code, opener, malformed) in cases {
        let analysis = RegexValidator::new().analyze(pattern);
        assert_eq!(analysis.facts.embedded_code.len(), 1, "unexpected facts for {pattern:?}");
        assert_eq!(analysis.facts.dynamic_regions.len(), 1, "unexpected regions for {pattern:?}");
        assert_eq!(analysis.malformed, malformed, "malformed state for {pattern:?}");

        let fact = &analysis.facts.embedded_code[0];
        let expected_start = pattern.find(construct).ok_or("missing expected construct")?;
        let expected_end = expected_start
            .checked_add(construct.len())
            .ok_or("expected construct range overflow")?;
        assert_eq!(fact.range.start, expected_start);
        assert_eq!(fact.range.end, expected_end);
        assert_eq!(pattern.get(fact.range.start..fact.range.end), Some(construct));
        assert!(pattern.is_char_boundary(fact.range.start));
        assert!(pattern.is_char_boundary(fact.range.end));

        let region = &analysis.facts.dynamic_regions[0];
        assert_eq!(region.kind, dynamic_kind);
        assert_eq!(region.range, fact.range);

        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == diagnostic_code)
            .ok_or("missing embedded-code diagnostic")?;
        assert_eq!(diagnostic.range.start, expected_start);
        assert_eq!(diagnostic.range.end, expected_start + opener.len());
        assert_eq!(pattern.get(diagnostic.range.start..diagnostic.range.end), Some(opener));
    }

    Ok(())
}

#[test]
fn escaped_quoted_commented_and_interpolated_text_is_not_embedded_code()
-> Result<(), Box<dyn std::error::Error>> {
    let patterns = [
        r#"\(?{ escaped })"#,
        r#"[(?{ inside a class })]"#,
        r#"\Q(?{ inside quoted literal })\E"#,
        r#"${runtime}"#,
        r#"(?# comment containing (?{ text }))"#,
        r#"(?x:# comment containing (?{ text }))"#,
    ];

    for pattern in patterns {
        let analysis = RegexValidator::new().analyze(pattern);
        assert!(
            analysis.facts.embedded_code.is_empty(),
            "false embedded-code fact for {pattern:?}"
        );
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != RegexDiagnosticCode::EmbeddedCodeImmediate
                    && diagnostic.code != RegexDiagnosticCode::EmbeddedCodeDeferred)
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
