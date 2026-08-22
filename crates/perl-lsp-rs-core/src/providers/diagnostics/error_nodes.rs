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
    use super::check_error_nodes;
    use perl_parser::Parser;
    use perl_parser_core::error_classifier::ErrorClassifier;
    use perl_tdd_support::must;

    #[test]
    fn recovered_parse_error_is_marked_fixable_at_the_error_node_boundary() {
        let source = "if () { 1 }";
        let ast = must(Parser::new(source).parse());
        let classifier = ErrorClassifier::new();
        let mut diagnostics = Vec::new();

        check_error_nodes(&ast, source, &classifier, &mut diagnostics);

        assert!(
            !diagnostics.is_empty(),
            "the malformed condition must reach the ERROR-node producer"
        );
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fixable));
    }
}
