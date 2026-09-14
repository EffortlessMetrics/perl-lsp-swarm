//! Heredoc anti-pattern detection diagnostics

use super::internal_types::Diagnostic;
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser::heredoc_anti_patterns::{
    AntiPattern, AntiPatternDetector, DetectionReport, DetectionStatus, DetectorState, Severity,
};

/// Detect heredoc anti-patterns in Perl source code.
///
/// Returns diagnostics for problematic heredoc patterns (eval strings,
/// dynamic delimiters, format blocks, etc.). Completeness comes from the
/// parser report: a partial or unavailable scan emits an informational
/// diagnostic so an empty finding list cannot masquerade as complete-clean.
pub fn detect_heredoc_antipatterns(source: &str) -> Vec<Diagnostic> {
    let report = AntiPatternDetector::new().detect_all_report(source);
    diagnostics_from_report(source, &report)
}

fn diagnostics_from_report(source: &str, report: &DetectionReport) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = report
        .diagnostics
        .iter()
        .cloned()
        .map(|d| {
            let offset = d.pattern.offset();
            let end_offset = (offset + 1).min(source.len());

            let severity = match d.severity {
                Severity::Error => DiagnosticSeverity::Error,
                Severity::Warning => DiagnosticSeverity::Warning,
                Severity::Info => DiagnosticSeverity::Information,
            };

            let code = antipattern_code(&d.pattern);

            Diagnostic {
                range: (offset, end_offset),
                severity,
                code: Some(code.to_string()),
                message: d.message,
                related_information: Vec::new(),
                tags: Vec::new(),
                fixable: false,
                critic_observation: None,
                suggestion: d.suggested_fix,
            }
        })
        .collect();

    if report.status != DetectionStatus::Complete {
        diagnostics.push(incomplete_scan_diagnostic(source, report));
    }

    diagnostics
}

fn incomplete_scan_diagnostic(source: &str, report: &DetectionReport) -> Diagnostic {
    let limited: Vec<&'static str> = report
        .detectors
        .iter()
        .filter(|obs| !matches!(obs.state, DetectorState::Complete))
        .map(|obs| obs.id.as_str())
        .collect();
    let end = source.len().min(1);
    Diagnostic {
        range: (0, end),
        severity: DiagnosticSeverity::Information,
        code: None,
        message: format!(
            "Heredoc anti-pattern analysis is {}; unavailable or limited detectors: {}",
            report.status.as_str(),
            limited.join(", ")
        ),
        related_information: Vec::new(),
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: None,
    }
}

fn antipattern_code(pattern: &AntiPattern) -> &'static str {
    match pattern {
        AntiPattern::FormatHeredoc { .. } => DiagnosticCode::HeredocInFormat.as_str(),
        AntiPattern::BeginTimeHeredoc { .. } => DiagnosticCode::HeredocInBegin.as_str(),
        AntiPattern::DynamicHeredocDelimiter { .. } => {
            DiagnosticCode::HeredocDynamicDelimiter.as_str()
        }
        AntiPattern::SourceFilterHeredoc { .. } => DiagnosticCode::HeredocInSourceFilter.as_str(),
        AntiPattern::RegexCodeBlockHeredoc { .. } => DiagnosticCode::HeredocInRegexCode.as_str(),
        AntiPattern::EvalStringHeredoc { .. } => DiagnosticCode::HeredocInEval.as_str(),
        AntiPattern::TiedHandleHeredoc { .. } => DiagnosticCode::HeredocTiedHandle.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::diagnostics::DiagnosticsProvider;
    use super::{DetectionReport, DetectionStatus, diagnostics_from_report};
    use perl_parser::Parser;
    use perl_parser::heredoc_anti_patterns::{
        DetectorFailureReason, DetectorId, DetectorObservation, DetectorState,
    };
    use std::sync::Arc;

    #[test]
    fn format_heredoc_diagnostic_reaches_provider_as_non_fixable_pl800() {
        let source = "format REPORT =\n<<'END'\nName: @<<<<\n$name\nEND\n.\n";
        let output = Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);
        let diagnostics =
            DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None);

        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some("PL800"))
            .expect("format heredoc must be reported by the provider as PL800");
        assert_eq!(diagnostic.code.as_deref(), Some("PL800"));
        assert!(!diagnostic.fixable);
    }

    #[test]
    fn partial_empty_report_is_not_projected_as_clean() {
        let report = DetectionReport {
            diagnostics: Vec::new(),
            detectors: vec![
                DetectorObservation {
                    id: DetectorId::FormatHeredoc,
                    state: DetectorState::Complete,
                },
                DetectorObservation {
                    id: DetectorId::SourceFilter,
                    state: DetectorState::Unavailable {
                        reason: DetectorFailureReason::PatternUnavailable {
                            pattern_ids: vec!["SOURCE_FILTER_PATTERN"],
                        },
                    },
                },
            ],
            status: DetectionStatus::Partial,
        };

        let projected = diagnostics_from_report("my $x = 1;\n", &report);
        assert!(
            !projected.is_empty(),
            "partial-empty analysis must remain observable at the LSP projection"
        );
        assert!(projected.iter().any(|diagnostic| {
            diagnostic.severity == perl_diagnostics::codes::DiagnosticSeverity::Information
                && diagnostic.message.contains("partial")
        }));
    }

    #[test]
    fn complete_clean_report_projects_no_diagnostics() {
        let report = DetectionReport {
            diagnostics: Vec::new(),
            detectors: vec![DetectorObservation {
                id: DetectorId::SourceFilter,
                state: DetectorState::Complete,
            }],
            status: DetectionStatus::Complete,
        };

        assert!(diagnostics_from_report("my $x = 1;\n", &report).is_empty());
    }
}
