//! PL406 projection of the shared local unreachable-statement analysis.

use super::super::internal_types::{Diagnostic, DiagnosticTag};
use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
use perl_parser_core::ast::Node;
use perl_semantic_analyzer::find_unreachable_statements;

/// Walk the AST and emit PL406 diagnostics for statements that cannot be
/// reached from the preceding sibling in the same local statement list.
pub fn check_unreachable_code(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.extend(find_unreachable_statements(root).into_iter().map(|statement| Diagnostic {
        range: (statement.range.start, statement.range.end),
        severity: DiagnosticSeverity::Hint,
        code: Some(DiagnosticCode::UnreachableCode.as_str().to_string()),
        message: "Unreachable code: this statement cannot be executed".to_string(),
        related_information: vec![],
        tags: vec![DiagnosticTag::Unnecessary],
        fixable: false,
        critic_observation: None,
        suggestion: Some("Remove unreachable code".to_string()),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_test_must::{must, must_some};

    #[test]
    fn recovered_syntax_preserves_fallthrough_at_the_diagnostic_boundary() {
        let mut parser = Parser::new("my $x = ; print \"live\";");
        let ast = must(parser.parse());
        assert!(!parser.errors().is_empty(), "fixture must exercise parser recovery");
        let mut diagnostics = Vec::new();
        check_unreachable_code(&ast, &mut diagnostics);
        assert!(diagnostics.is_empty(), "recovery cannot fabricate PL406: {diagnostics:?}");
    }

    #[test]
    fn pl406_preserves_ordered_ranges_and_output_projection() {
        let source = "sub f { return; my $first = 1; my $second = 2; }";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        assert!(parser.errors().is_empty());
        let mut diagnostics = Vec::new();
        check_unreachable_code(&ast, &mut diagnostics);
        let expected_ranges: Vec<_> = ["my $first = 1", "my $second = 2"]
            .into_iter()
            .map(|anchor| {
                let start = must_some(source.find(anchor));
                (start, start + anchor.len())
            })
            .collect();
        assert_eq!(diagnostics.iter().map(|d| d.range).collect::<Vec<_>>(), expected_ranges);
        for diagnostic in diagnostics {
            assert_eq!(diagnostic.code.as_deref(), Some("PL406"));
            assert_eq!(diagnostic.severity, DiagnosticSeverity::Hint);
            assert_eq!(diagnostic.message, "Unreachable code: this statement cannot be executed");
            assert_eq!(diagnostic.tags, vec![DiagnosticTag::Unnecessary]);
            assert!(diagnostic.related_information.is_empty());
            assert!(!diagnostic.fixable);
            assert!(diagnostic.critic_observation.is_none());
            assert_eq!(diagnostic.suggestion.as_deref(), Some("Remove unreachable code"));
        }
    }
}
