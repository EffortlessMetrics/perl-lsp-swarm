use std::error::Error;

use perl_parser_core::syntax::{
    error::ParseDiagnosticSeverity, quote::RegexFamilyOperator,
    regex_analysis::RegexAnalysisAvailability,
};
use perl_parser_core::{
    Node, NodeKind, Token, TokenKind, TokenStream, parse_source_with_regex_analysis,
    parse_tokens_with_regex_analysis,
};
use perl_regex::analyzer::{FeatureState, PatternControlResolution};

fn lex(source: &str) -> Result<Vec<Token>, Box<dyn Error>> {
    let mut stream = TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next()?;
        if token.kind == TokenKind::Eof {
            break;
        }
        tokens.push(token);
    }
    Ok(tokens)
}

fn collect_embedded_flags(node: &Node, flags: &mut Vec<bool>) {
    match &node.kind {
        NodeKind::Regex { has_embedded_code, .. }
        | NodeKind::Match { has_embedded_code, .. }
        | NodeKind::Substitution { has_embedded_code, .. } => {
            flags.push(*has_embedded_code);
        }
        _ => {}
    }
    node.for_each_child(|child| collect_embedded_flags(child, flags));
}

#[test]
fn retains_source_bound_pattern_analysis_with_exact_geometry() -> Result<(), Box<dyn Error>> {
    let source = r"my $x = /(?<id>a)\k<id>/;";
    let output = parse_source_with_regex_analysis(source);

    assert!(output.regex_analysis.source_matches(source));
    assert_eq!(output.regex_analysis.len(), 1);
    assert_eq!(output.regex_analysis.analysis_invocations(), 1);

    let record = output.regex_analysis.records.first().ok_or("missing regex record")?;
    assert_eq!(record.operator, Some(RegexFamilyOperator::BareMatch));
    assert_eq!(record.availability, RegexAnalysisAvailability::Analyzed);
    let geometry = record.geometry.as_ref().ok_or("missing geometry")?;
    assert_eq!(geometry.pattern.text, r"(?<id>a)\k<id>");
    assert_eq!(
        source.get(geometry.pattern.range.start..geometry.pattern.range.end),
        Some(geometry.pattern.text.as_str())
    );
    let pattern = record.pattern.as_ref().ok_or("missing retained analysis")?;
    assert_eq!(pattern.controls.captures.declarations.len(), 1);
    assert_eq!(pattern.controls.facts.len(), 1);
    assert!(matches!(
        &pattern.controls.facts[0].resolution,
        PatternControlResolution::ProfileDependent { known_targets }
            if known_targets.len() == 1
    ));
    assert!(!record.is_complete());
    Ok(())
}

#[test]
fn derives_lexical_source_utf8_and_lossless_modifier_profile() -> Result<(), Box<dyn Error>> {
    let source = "use utf8; my $rx = qr{(?<café>\\w+)}xx;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing qr record")?;

    assert_eq!(record.operator, Some(RegexFamilyOperator::QuoteRegex));
    assert_eq!(record.profile.source_utf8, FeatureState::Enabled);
    assert_eq!(record.profile.regex.enhanced_xx, FeatureState::Disabled);
    assert_eq!(record.profile.regex.perl_version, None);
    let modifiers = record.modifiers.as_ref().ok_or("missing modifiers")?;
    assert_eq!(modifiers.sequence.raw, "xx");
    assert_eq!(modifiers.tokens.len(), 2);
    Ok(())
}

#[test]
fn transliteration_retains_geometry_without_regex_body_analysis() -> Result<(), Box<dyn Error>> {
    let source = "tr/a-z/A-Z/cds;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing transliteration record")?;

    assert_eq!(record.operator, Some(RegexFamilyOperator::Transliteration));
    assert_eq!(record.availability, RegexAnalysisAvailability::TransliterationNotRegex);
    assert!(record.pattern.is_none());
    assert_eq!(output.regex_analysis.analysis_invocations(), 0);
    let modifiers = record.modifiers.as_ref().ok_or("missing transliteration modifiers")?;
    assert!(modifiers.effective.transliteration.complement);
    assert!(modifiers.effective.transliteration.delete);
    assert!(modifiers.effective.transliteration.squash);
    Ok(())
}

#[test]
fn substitution_e_and_inline_code_project_one_ast_compatibility_fact() -> Result<(), Box<dyn Error>>
{
    let source = "s/(?{ side_effect() })foo/bar/e;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing substitution record")?;

    assert_eq!(record.operator, Some(RegexFamilyOperator::Substitution));
    assert!(record.has_embedded_code());
    let mut flags = Vec::new();
    collect_embedded_flags(&output.ast, &mut flags);
    assert_eq!(flags, vec![true]);
    Ok(())
}

#[test]
fn multiple_findings_survive_in_source_order_without_duplicate_analysis()
-> Result<(), Box<dyn Error>> {
    let source = r"my $x = /(?<id>a)(a+)+\k<missing>/;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing regex record")?;
    let pattern = record.pattern.as_ref().ok_or("missing pattern analysis")?;

    assert!(pattern.structural.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == perl_regex::validator::RegexDiagnosticCode::NestedQuantifierRisk
    }));
    assert!(pattern.controls.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == perl_regex::analyzer::PatternControlDiagnosticCode::UnresolvedReference
    }));
    assert_eq!(output.regex_analysis.analysis_invocations(), 1);

    let locations = output
        .diagnostics
        .iter()
        .filter_map(perl_parser_core::ParseError::location)
        .collect::<Vec<_>>();
    assert!(locations.windows(2).all(|window| window[0] <= window[1]));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.severity() == ParseDiagnosticSeverity::Advisory })
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.severity() == ParseDiagnosticSeverity::Blocking })
    );
    Ok(())
}

#[test]
fn nested_quantifier_advisory_selects_the_outer_quantifier() -> Result<(), Box<dyn Error>> {
    let source = "my $ok = /^(a+)+$/;";
    let output = parse_source_with_regex_analysis(source);
    let expected = source.find(")+").ok_or("outer quantifier not found")? + 1;

    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.location() == Some(expected)
            && diagnostic.severity() == ParseDiagnosticSeverity::Advisory
    }));
    Ok(())
}

#[test]
fn fresh_and_prelexed_entry_points_retain_identical_tables() -> Result<(), Box<dyn Error>> {
    let source = r"my $rx = qr{(?<id>\d+)}i; my $ok = /(?<x>a)\k<x>/;";
    let fresh = parse_source_with_regex_analysis(source);
    let prelexed = parse_tokens_with_regex_analysis(lex(source)?, source);

    assert_eq!(fresh.ast.to_sexp(), prelexed.ast.to_sexp());
    assert_eq!(fresh.regex_analysis, prelexed.regex_analysis);
    assert_eq!(fresh.diagnostics, prelexed.diagnostics);
    Ok(())
}

#[test]
fn source_edit_changes_digest_and_record_geometry() -> Result<(), Box<dyn Error>> {
    let first_source = "my $x = /a/;";
    let second_source = "my $prefix = 'é'; my $x = /ab/;";
    let first = parse_source_with_regex_analysis(first_source);
    let second = parse_source_with_regex_analysis(second_source);

    assert_ne!(first.regex_analysis.source_digest, second.regex_analysis.source_digest);
    assert!(!first.regex_analysis.source_matches(second_source));
    assert!(!second.regex_analysis.source_matches(first_source));
    let first_range = first.regex_analysis.records[0].full_range;
    let second_range = second.regex_analysis.records[0].full_range;
    assert_ne!(first_range, second_range);
    Ok(())
}

#[test]
fn consecutive_parses_do_not_share_pending_geometry() -> Result<(), Box<dyn Error>> {
    let first = parse_source_with_regex_analysis("my $x = /a/; my $y = /b/;");
    let second = parse_source_with_regex_analysis("my $z = /c/;");

    assert_eq!(first.regex_analysis.len(), 2);
    assert_eq!(second.regex_analysis.len(), 1);
    let geometry = second.regex_analysis.records[0].geometry.as_ref().ok_or("missing geometry")?;
    assert_eq!(geometry.pattern.text, "c");
    Ok(())
}

#[test]
fn malformed_pattern_is_retained_with_explicit_incompleteness() -> Result<(), Box<dyn Error>> {
    let source = "my $x = /(?<id>a/;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing malformed record")?;

    assert!(!record.is_complete());
    assert_eq!(record.availability, RegexAnalysisAvailability::Analyzed);
    assert!(record.pattern.as_ref().is_some_and(|pattern| pattern.controls.status.malformed));
    assert!(output.regex_analysis.source_matches(source));
    Ok(())
}
