//! Heredoc anti-pattern detection diagnostics

use super::internal_types::Diagnostic;
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser::heredoc_anti_patterns::{AntiPattern, AntiPatternDetector, Severity};

/// Detect heredoc anti-patterns in Perl source code.
///
/// Returns diagnostics for problematic heredoc patterns (eval strings,
/// dynamic delimiters, format blocks, etc.)
pub fn detect_heredoc_antipatterns(source: &str) -> Vec<Diagnostic> {
    let detector = AntiPatternDetector::new();
    let raw = detector.detect_all(source);

    raw.into_iter()
        .map(|d| {
            let offset = extract_offset(&d.pattern);
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
        .collect()
}

fn extract_offset(pattern: &AntiPattern) -> usize {
    match pattern {
        AntiPattern::FormatHeredoc { location, .. }
        | AntiPattern::BeginTimeHeredoc { location, .. }
        | AntiPattern::DynamicHeredocDelimiter { location, .. }
        | AntiPattern::SourceFilterHeredoc { location, .. }
        | AntiPattern::RegexCodeBlockHeredoc { location, .. }
        | AntiPattern::EvalStringHeredoc { location, .. }
        | AntiPattern::TiedHandleHeredoc { location, .. } => location.offset,
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
    use perl_parser::Parser;
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

    fn provider_codes(source: &str) -> Vec<String> {
        let output = Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);
        DiagnosticsProvider::new()
            .get_diagnostics(&ast, &output.diagnostics, source, None)
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn multiline_regex_code_block_heredoc_reaches_provider_as_pl804() {
        // #3597: the newline horizon introduced by #3568 made this construct
        // invisible to the user even though it is the shape real Perl uses.
        let source = "m/pattern(?{\n    print <<'MATCH';\nMatch text\nMATCH\n})/;\n";

        let codes = provider_codes(source);
        assert!(
            codes.iter().any(|code| code == "PL804"),
            "multi-line regex code block heredoc must surface as PL804; got {codes:?}"
        );
    }

    #[test]
    fn multiline_eval_string_heredoc_reaches_provider_as_pl805() {
        let source = "eval 'print <<\"EVAL\";\nbody text\nEVAL\n';\n";

        let codes = provider_codes(source);
        assert!(
            codes.iter().any(|code| code == "PL805"),
            "multi-line eval string heredoc must surface as PL805; got {codes:?}"
        );
    }

    #[test]
    fn ordinary_perl_surfaces_no_heredoc_antipattern_codes() {
        // Negative control: the two assertions above are only meaningful if a
        // clean file stays free of PL80x codes.
        let source = "use strict;\nuse warnings;\n\nsub add {\n    my ($a, $b) = @_;\n    return $a + $b;\n}\n";

        let codes = provider_codes(source);
        assert!(
            !codes.iter().any(|code| code.starts_with("PL80")),
            "ordinary Perl must not surface heredoc anti-pattern codes; got {codes:?}"
        );
    }
}
