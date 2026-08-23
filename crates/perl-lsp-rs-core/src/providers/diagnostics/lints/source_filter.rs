//! Source-filter module warning.
//!
//! Warns when a file loads a known source-filter module via `use` or `no`.
//! Source filters (`Filter::Simple`, `Filter::Util::Call`, and friends)
//! rewrite the program text before Perl parses it, so any static analysis
//! performed downstream — completion, navigation, diagnostics — is working
//! against source that no longer reflects what the interpreter will actually
//! compile. Flagging the import lets editors surface that caveat instead of
//! silently reporting results that may be wrong.
//!
//! The risky-module decision is made once, at parse time, by
//! [`Parser::is_filter_module`](perl_parser_core) and recorded as the
//! `has_filter_risk` flag on each [`NodeKind::Use`] / [`NodeKind::No`] node.
//! This lint only surfaces that precomputed flag; it does not re-derive the
//! module list, so the two never drift.
//!
//! # Diagnostic codes
//!
//! | Code | Severity | Description |
//! |------|----------|-------------|
//! | `PL702` | Warning | `use`/`no` of a known source-filter module |

use super::super::internal_types::{Diagnostic, RelatedInformation};
use super::super::walker::walk_node;
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::{Node, NodeKind};

const EXPLANATION: &str = "Source filters rewrite source code before it's parsed. \
Static analysis cannot reliably predict the state of the code after filtering.";

/// Warn on every `use`/`no` of a module the parser flagged as a source filter.
pub fn check_source_filter_risk(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    walk_node(root, &mut |node| {
        let module = match &node.kind {
            NodeKind::Use { module, has_filter_risk, .. }
            | NodeKind::No { module, has_filter_risk, .. }
                if *has_filter_risk =>
            {
                module
            }
            _ => return,
        };

        diagnostics.push(Diagnostic {
            range: (node.location.start, node.location.end),
            severity: DiagnosticSeverity::Warning,
            code: Some(DiagnosticCode::SourceFilterModule.as_str().to_string()),
            message: format!("'{module}' is a source filter module"),
            related_information: vec![RelatedInformation {
                location: (node.location.start, node.location.end),
                message: EXPLANATION.to_string(),
            }],
            tags: Vec::new(),
            fixable: false,
            suggestion: Some(
                "Avoid source filters; prefer modern Perl features or Devel::Declare-style \
                alternatives that don't rewrite source before parsing."
                    .to_string(),
            ),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_tdd_support::must;

    fn filter_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let mut diags = Vec::new();
        check_source_filter_risk(&ast, &mut diags);
        diags
    }

    fn is_pl702(d: &Diagnostic) -> bool {
        d.code.as_deref() == Some("PL702")
    }

    #[test]
    fn each_known_filter_module_warns() {
        // Mirrors Parser::is_filter_module's list exactly.
        for module in [
            "Filter",
            "Filter::Util::Call",
            "Filter::Simple",
            "Filter::cpp",
            "Filter::exec",
            "Filter::sh",
            "Filter::tee",
            "Filter::decrypt",
        ] {
            let source = format!("use {module};\n");
            let diags = filter_diags(&source);
            assert!(diags.iter().any(is_pl702), "`use {module};` should raise PL702: {diags:?}");
        }
    }

    #[test]
    fn non_filter_module_does_not_warn() {
        let diags = filter_diags("use strict;\nuse warnings;\nuse List::Util qw(sum);\n");
        assert!(!diags.iter().any(is_pl702), "ordinary modules must not raise PL702: {diags:?}");
    }

    #[test]
    fn no_statement_for_filter_module_warns() {
        // `has_filter_risk` is stamped on `no` nodes too.
        let diags = filter_diags("no Filter::Simple;\n");
        assert!(diags.iter().any(is_pl702), "`no Filter::Simple;` should raise PL702: {diags:?}");
    }

    #[test]
    fn range_is_non_degenerate() {
        let diags = filter_diags("use Filter::Simple;\n");
        let d = diags.iter().find(|d| is_pl702(d)).expect("expected a PL702 diagnostic");
        assert!(d.range.0 < d.range.1, "diagnostic range should span the statement: {d:?}");
    }
}
