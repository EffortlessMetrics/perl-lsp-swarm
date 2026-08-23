//! ERROR node classification and diagnostic generation
//!
//! This module provides functionality for checking ERROR nodes in the AST
//! and classifying them into appropriate diagnostic messages.

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::error_classifier::ErrorClassifier;

use super::internal_types::{Diagnostic, RelatedInformation};
use super::walker::walk_node;
use perl_diagnostics::codes::DiagnosticSeverity;

/// Check for ERROR nodes in the AST and classify them
///
/// This function walks the AST looking for ERROR nodes, classifies them
/// using the error classifier, and generates appropriate diagnostics with
/// helpful suggestions and explanations.
#[allow(dead_code)]
pub fn check_error_nodes(
    node: &Node,
    source: &str,
    error_classifier: &ErrorClassifier,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_node(node, &mut |n| {
        if let NodeKind::Error { message, .. } = &n.kind {
            let error_kind = error_classifier.classify(n, source);
            let diagnostic_message = error_classifier.get_diagnostic_message(&error_kind);
            let suggestion = error_classifier.get_suggestion(&error_kind);
            let explanation = error_classifier.get_explanation(&error_kind);

            let mut full_message = diagnostic_message.clone();
            if !message.is_empty() {
                full_message.push_str(&format!(": {}", message));
            }

            let start = n.location.start;
            let end = n.location.end.min(source.len());

            // Build related information with suggestion and explanation
            let mut related_info = Vec::new();
            if let Some(ref sugg) = suggestion {
                related_info.push(RelatedInformation {
                    location: (start, end),
                    message: format!("💡 {}", sugg),
                });
            }
            if let Some(exp) = explanation {
                related_info.push(RelatedInformation {
                    location: (start, end),
                    message: format!("ℹ️ {}", exp),
                });
            }

            diagnostics.push(Diagnostic {
                range: (start, end),
                severity: DiagnosticSeverity::Error,
                code: Some(DiagnosticCode::ParseError.as_str().to_string()),
                message: full_message,
                related_information: related_info,
                tags: Vec::new(),
                suggestion,
                fixable: true,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::diagnostics::DiagnosticsProvider;
    use super::{DiagnosticCode, check_error_nodes};
    use perl_parser::Parser;
    use perl_parser_core::error_classifier::ErrorClassifier;
    use std::sync::Arc;

    #[test]
    fn error_node_helper_emits_fixable_parse_diagnostic() {
        // Unit-scoped coverage of check_error_nodes; provider conversion is covered separately below.
        let source = "if () { 1 }";
        let ast = Parser::new(source)
            .parse()
            .expect("malformed condition must produce the established ERROR-node AST");
        let classifier = ErrorClassifier::new();
        let mut diagnostics = Vec::new();

        check_error_nodes(&ast, source, &classifier, &mut diagnostics);

        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code.as_deref() == Some(DiagnosticCode::ParseError.as_str())
            })
            .expect("the ERROR node must emit a ParseError diagnostic");
        assert!(diagnostic.fixable);
    }

    #[test]
    fn malformed_parse_output_reaches_provider_as_fixable_parse_diagnostic() {
        let source = "if () { 1 }";
        let output = Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);
        let diagnostics =
            DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None);

        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some("PL001"))
            .expect("malformed parser output must reach the provider as PL001");

        assert!(diagnostic.fixable);
    }
}
