//! Regression for #14754: retained regex analysis must thread lexical `use VERSION`
//! into `profile_at` so version-gated modifier and capture findings can fire.

use std::error::Error;

use perl_parser_core::{
    ParseError, engine::regex_retention::RegexParseOutput, parse_source_with_regex_analysis,
    syntax::regex_analysis::RegexAnalysisRecord,
};
use perl_regex::analyzer::{CaptureDiagnosticCode, CaptureMode, FeatureState};
use perl_regex::validator::RegexDiagnosticCode;

fn match_record(output: &RegexParseOutput) -> Result<&RegexAnalysisRecord, Box<dyn Error>> {
    output
        .regex_analysis
        .records
        .iter()
        .find(|record| record.geometry.is_some())
        .ok_or_else(|| "missing retained regex record".into())
}

#[test]
fn old_use_version_reports_non_capturing_modifier_and_withholds_effect()
-> Result<(), Box<dyn Error>> {
    let source = "use v5.8;\nmy $lead = 1;\nmy $x = $s =~ m/(foo)/n;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let modifiers = record.modifiers.as_ref().ok_or("missing modifier analysis")?;

    assert_eq!(
        record.profile.regex.perl_version,
        Some(perl_regex::analyzer::PerlVersion::new(5, 8))
    );
    assert!(
        modifiers.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == RegexDiagnosticCode::ModifierRequiresPerlVersion
        })
    );
    assert_eq!(modifiers.effective.captures, CaptureMode::CapturingByDefault);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            ParseError::SyntaxError { message, location }
                if message.contains("Regex modifier requires a newer Perl version") && *location == 46
        )
    }));
    Ok(())
}

#[test]
fn current_use_version_admits_non_capturing_modifier() -> Result<(), Box<dyn Error>> {
    let source = "use v5.36;\nmy $lead = 1;\nmy $x = $s =~ m/(foo)/n;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let modifiers = record.modifiers.as_ref().ok_or("missing modifier analysis")?;

    assert_eq!(
        record.profile.regex.perl_version,
        Some(perl_regex::analyzer::PerlVersion::new(5, 36))
    );
    assert!(
        modifiers
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != RegexDiagnosticCode::ModifierRequiresPerlVersion)
    );
    assert_eq!(modifiers.effective.captures, CaptureMode::NonCapturingByDefault);
    Ok(())
}

#[test]
fn missing_use_version_leaves_modifier_requirement_unknown_without_diagnostic()
-> Result<(), Box<dyn Error>> {
    let source = "my $lead = 1;\nmy $x = $s =~ m/(foo)/n;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let modifiers = record.modifiers.as_ref().ok_or("missing modifier analysis")?;

    assert_eq!(record.profile.regex.perl_version, None);
    assert!(modifiers.diagnostics.is_empty());
    assert_eq!(modifiers.effective.captures, CaptureMode::NonCapturingByDefault);
    Ok(())
}

#[test]
fn old_use_version_reports_named_capture_version_requirement() -> Result<(), Box<dyn Error>> {
    let source = "use v5.8;\nmy $lead = 1;\nmy $re = qr/(?<name>x)/;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let pattern = record.pattern.as_ref().ok_or("missing pattern analysis")?;

    assert!(
        pattern
            .controls
            .captures
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == CaptureDiagnosticCode::RequiresPerlVersion })
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            ParseError::SyntaxError { message, location }
                if message.contains("Regex capture syntax requires a newer Perl version")
                    && *location == 39
        )
    }));
    Ok(())
}

#[test]
fn current_use_version_admits_named_capture_analysis() -> Result<(), Box<dyn Error>> {
    let source = "use v5.36;\nmy $lead = 1;\nmy $re = qr/(?<name>x)/;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let pattern = record.pattern.as_ref().ok_or("missing pattern analysis")?;

    assert!(
        pattern
            .controls
            .captures
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != CaptureDiagnosticCode::RequiresPerlVersion)
    );
    assert_eq!(pattern.controls.captures.declarations.len(), 1);
    Ok(())
}

#[test]
fn missing_use_version_leaves_named_capture_requirement_unknown_without_diagnostic()
-> Result<(), Box<dyn Error>> {
    let source = "my $lead = 1;\nmy $re = qr/(?<name>x)/;";
    let output = parse_source_with_regex_analysis(source);
    let record = match_record(&output)?;
    let pattern = record.pattern.as_ref().ok_or("missing pattern analysis")?;

    assert_eq!(record.profile.regex.perl_version, None);
    assert!(pattern.controls.captures.diagnostics.is_empty());
    assert_eq!(pattern.controls.captures.declarations.len(), 1);
    Ok(())
}

#[test]
fn utf8_without_version_declaration_keeps_unknown_perl_version() -> Result<(), Box<dyn Error>> {
    let source = "use utf8; my $rx = qr{(?<café>\\w+)}xx;";
    let output = parse_source_with_regex_analysis(source);
    let record = output.regex_analysis.records.first().ok_or("missing qr record")?;

    assert_eq!(record.profile.source_utf8, FeatureState::Enabled);
    assert_eq!(record.profile.regex.perl_version, None);
    Ok(())
}
