//! PL406 projection of the shared local unreachable-code analysis.
//!
//! The semantic analyzer owns the traversal and its disposition registry.
//! This module retains the public diagnostic entry point and presentation.

use super::super::internal_types::{Diagnostic, DiagnosticTag};
use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
use perl_parser_core::Node;
use perl_semantic_analyzer::collect_unreachable_spans;

/// Emit PL406 diagnostics for the shared analyzer's ordered unreachable spans.
pub fn check_unreachable_code(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.extend(collect_unreachable_spans(root).into_iter().map(|span| Diagnostic {
        range: (span.start, span.end),
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
    use perl_tdd_support::{must, must_some};

    #[test]
    fn pl406_keeps_canonical_tag_and_suggestion() {
        let source = "sub f { return; my $x = 1; }";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        assert!(parser.errors().is_empty());
        let mut diagnostics = Vec::new();
        check_unreachable_code(&ast, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = must_some(diagnostics.first());
        let start = must_some(source.find("my $x = 1"));
        assert_eq!(diagnostic.range, (start, start + "my $x = 1".len()));
        assert_eq!(diagnostic.code.as_deref(), Some("PL406"));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Hint);
        assert_eq!(diagnostic.message, "Unreachable code: this statement cannot be executed");
        assert_eq!(diagnostic.tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diagnostic.suggestion.as_deref(), Some("Remove unreachable code"));
        assert!(!diagnostic.fixable);
        assert!(diagnostic.related_information.is_empty());
        assert!(diagnostic.critic_observation.is_none());
    }
}
