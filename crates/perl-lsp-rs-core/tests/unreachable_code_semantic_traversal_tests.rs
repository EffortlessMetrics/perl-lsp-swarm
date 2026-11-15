//! Consumer projections of the shared lower-owned unreachable-code facts.

use perl_diagnostics::codes::DiagnosticSeverity;
use perl_lsp_rs_core::providers::diagnostics::{
    DiagnosticTag, unreachable_code::check_unreachable_code,
};
use perl_lsp_rs_core::tooling::perl_critic::{
    CriticConfig, CriticContext, CriticRule, Severity, UnreachableCodeRule,
};
use perl_parser_core::Parser;
use perl_semantic_analyzer::collect_unreachable_spans;

#[test]
fn diagnostic_and_native_projections_preserve_ordered_shared_spans()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, anchors) in [
        ("sub f { return; my $a = 1; my $b = 2; }", vec!["my $a = 1", "my $b = 2"]),
        ("sub f { my $g = sub { return; my $inner = 1; }; my $outer = 2; }", vec!["my $inner = 1"]),
        ("sub f { return if $condition; my $live = 1; }", vec![]),
        ("sub f { eval { die 'stop'; my $inner = 1; }; my $outer = 2; }", vec!["my $inner = 1"]),
    ] {
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        assert!(parser.errors().is_empty(), "fixture must parse cleanly");
        let expected = anchors
            .iter()
            .map(|anchor| {
                let start = source.find(anchor).ok_or("missing expected source anchor")?;
                Ok((start, start + anchor.len()))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        let spans = collect_unreachable_spans(&ast);
        assert_eq!(spans.iter().map(|span| (span.start, span.end)).collect::<Vec<_>>(), expected);

        let mut diagnostics = Vec::new();
        check_unreachable_code(&ast, &mut diagnostics);
        assert_eq!(
            diagnostics.iter().map(|diagnostic| diagnostic.range).collect::<Vec<_>>(),
            expected
        );
        for diagnostic in diagnostics {
            assert_eq!(diagnostic.code.as_deref(), Some("PL406"));
            assert_eq!(diagnostic.severity, DiagnosticSeverity::Hint);
            assert_eq!(diagnostic.message, "Unreachable code: this statement cannot be executed");
            assert_eq!(diagnostic.tags, vec![DiagnosticTag::Unnecessary]);
            assert_eq!(diagnostic.suggestion.as_deref(), Some("Remove unreachable code"));
            assert!(!diagnostic.fixable);
            assert!(diagnostic.related_information.is_empty());
            assert!(diagnostic.critic_observation.is_none());
        }

        let config = CriticConfig::default();
        let context = CriticContext::new(source, &ast, &config);
        let mut findings = Vec::new();
        UnreachableCodeRule.check(&context, &mut findings);
        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.range.start.byte, finding.range.end.byte))
                .collect::<Vec<_>>(),
            expected
        );
        for finding in findings {
            assert_eq!(finding.rule_id, "native.common.unreachable_code");
            assert_eq!(finding.suppression_key, "native.common.unreachable_code");
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.message, "Unreachable code: this statement cannot be executed");
            assert!(finding.related.is_empty());
            assert!(finding.fix.is_some(), "native remediation remains available");
        }
    }
    Ok(())
}

#[test]
fn recovered_input_preserves_conservative_consumer_fallthrough()
-> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $x = ; print \"live\";");
    let ast = parser.parse()?;
    assert!(!parser.errors().is_empty(), "fixture must exercise parser recovery");
    assert!(collect_unreachable_spans(&ast).is_empty());
    let mut diagnostics = Vec::new();
    check_unreachable_code(&ast, &mut diagnostics);
    assert!(diagnostics.is_empty());
    Ok(())
}

#[test]
fn compatibility_registry_reexports_the_identical_lower_types() {
    let lower = perl_semantic_analyzer::all_pl406_dispositions();
    let old: &[perl_lsp_rs_core::providers::diagnostics::unreachable_code_disposition::Pl406Disposition] = lower;
    assert_eq!(old, perl_lsp_rs_core::providers::diagnostics::unreachable_code_disposition::all_pl406_dispositions());
}
