//! Common mistakes lint checks
//!
//! This module provides functionality for detecting common mistakes in Perl code
//! such as assignment in conditions and comparing with undef.
//!
//! # Diagnostic codes
//!
//! | Code | Severity | Description |
//! |------|----------|-------------|
//! | `assignment-in-condition` | Warning | `=` in `if`/`while` condition (likely meant `==`) |
//! | `numeric-undef` | Warning | `==`/`!=` with potentially undefined value |
//! | `PL400` | Warning | Bareword filehandle in `open` call |

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::ast::{Node, NodeKind};
use perl_semantic_analyzer::symbol::{SymbolKind, SymbolTable};

use super::super::internal_types::{Diagnostic, RelatedInformation};
use super::super::walker::walk_node;
use crate::tooling::perl_critic::{BuiltInCriticObservation, Severity};
use perl_diagnostics::codes::DiagnosticSeverity;

/// Check for common mistakes
///
/// This function walks the AST looking for common mistakes such as:
/// - Assignment in condition (should be comparison)
/// - Using == or != with potentially undefined values
pub fn check_common_mistakes(
    node: &Node,
    symbol_table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_node(node, &mut |n| {
        match &n.kind {
            // Check for assignment in condition
            NodeKind::If { condition, elsif_branches, .. } => {
                check_assignment_in_condition(condition, diagnostics);
                for (elsif_condition, _) in elsif_branches {
                    check_assignment_in_condition(elsif_condition, diagnostics);
                }
            }
            NodeKind::While { condition, .. } => {
                check_assignment_in_condition(condition, diagnostics);
            }
            NodeKind::For { condition: Some(condition), .. } => {
                check_assignment_in_condition(condition, diagnostics);
            }
            NodeKind::StatementModifier { modifier, condition, .. } => {
                if matches!(modifier.as_str(), "if" | "unless" | "while" | "until") {
                    check_assignment_in_condition(condition, diagnostics);
                }
            }

            // Check for == or != with undef
            NodeKind::Binary { op, left, right }
                if (op == "==" || op == "!=")
                    && (might_be_undef(left, symbol_table)
                        || might_be_undef(right, symbol_table)) =>
            {
                // The emitter chooses the reviewed PL404 shape at the
                // syntax branch that observed it (#11918): a literal
                // `undef` operand is the literal shape (the reviewed
                // native alias `native.common.undef_comparison` covers
                // exactly that); an unresolved-variable operand is the
                // data-flow shape, which deliberately has no native
                // alias and stays a distinct finding.
                let literal_undef =
                    matches!(left.kind, NodeKind::Undef) || matches!(right.kind, NodeKind::Undef);
                let range = (n.location.start, n.location.end);
                let message = format!(
                    "Using '{}' with potentially undefined value -- use 'defined()' to check first",
                    op
                );
                const UNDEF_GUARD_SUGGESTION: &str =
                    "Guard with 'defined($var)' or use the '//' (defined-or) operator";
                const UNDEF_RELATED_EXPLANATION: &str =
                    "Consider using 'defined' check or '//' operator";
                let observation = if literal_undef {
                    BuiltInCriticObservation::pl404_literal_undef_comparison(
                        Severity::Stern,
                        range,
                        message.clone(),
                        Some(UNDEF_RELATED_EXPLANATION.to_string()),
                    )
                } else {
                    BuiltInCriticObservation::pl404_potentially_undef_comparison(
                        Severity::Stern,
                        range,
                        message.clone(),
                        Some(UNDEF_RELATED_EXPLANATION.to_string()),
                    )
                }
                // #12004: the observation carries the ordinary row's
                // exact user-visible remediation so retirement cannot
                // drop it. Shared bindings keep the copies identical.
                .with_suggestion(UNDEF_GUARD_SUGGESTION)
                .with_related_information(range, UNDEF_RELATED_EXPLANATION.to_string());
                diagnostics.push(Diagnostic {
                    range,
                    severity: DiagnosticSeverity::Warning,
                    code: Some(DiagnosticCode::NumericComparisonWithUndef.as_str().to_string()),
                    message,
                    related_information: vec![RelatedInformation {
                        location: range,
                        message: UNDEF_RELATED_EXPLANATION.to_string(),
                    }],
                    tags: Vec::new(),
                    fixable: false,
                    critic_observation: Some(observation),
                    suggestion: Some(UNDEF_GUARD_SUGGESTION.to_string()),
                });
            }
            NodeKind::FunctionCall { name, args } => {
                check_bareword_filehandle(name, args, n, diagnostics);
            }

            _ => {}
        }
    });
}

fn check_bareword_filehandle(
    function_name: &str,
    args: &[Node],
    node: &Node,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if function_name != "open" {
        return;
    }

    let open_args: &[Node] = if args.len() == 1 {
        if let NodeKind::ArrayLiteral { elements } = &args[0].kind { elements } else { args }
    } else {
        args
    };

    if open_args.len() < 2 {
        return;
    }

    let NodeKind::Identifier { name } = &open_args[0].kind else {
        return;
    };

    if matches!(name.as_str(), "STDIN" | "STDOUT" | "STDERR" | "ARGV" | "ARGVOUT" | "DATA") {
        return;
    }

    diagnostics.push(Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::BarewordFilehandle.as_str().to_string()),
        message: "Use lexical filehandles instead of bareword filehandles".to_string(),
        related_information: vec![RelatedInformation {
            location: (node.location.start, node.location.end),
            message:
                "Bareword filehandles are global and can lead to accidental reuse across scopes"
                    .to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some("Use lexical filehandle: open(my $fh, ... )".to_string()),
    });
}

/// Check for assignment in condition (common mistake)
fn check_assignment_in_condition(condition: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let is_assignment = matches!(
        &condition.kind,
        NodeKind::Binary { op, .. } if op == "="
    ) || matches!(&condition.kind, NodeKind::Assignment { .. });
    if !is_assignment {
        return;
    }
    let range = (condition.location.start, condition.location.end);
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::AssignmentInCondition.as_str().to_string()),
        message: "Assignment in condition - did you mean '=='?".to_string(),
        related_information: vec![
            RelatedInformation {
                location: range,
                message: "Suggestion: Use '==' for comparison or 'eq' for string comparison"
                    .to_string(),
            },
            RelatedInformation {
                location: range,
                message: "Note: Assignment in conditions is usually a mistake. If intentional, wrap in parentheses: if (($x = value))".to_string(),
            },
        ],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some("Replace '=' with '==' for numeric comparison or 'eq' for string comparison".to_string()),
    });
}

/// Check if a node might evaluate to undef
fn might_be_undef(node: &Node, symbol_table: &SymbolTable) -> bool {
    match &node.kind {
        NodeKind::Variable { name, .. } => {
            // Resolve the scope that actually encloses this variable use
            // (not the hard-coded global scope) so a `my` declared inside a
            // sub/block is visible for the rest of that lexical scope, per
            // perlsub/perlsyn. Falling back to global scope 0 -- the prior
            // behavior -- would make any sub-local lexical look undefined.
            let enclosing_scope = symbol_table.scope_at_offset(node.location.start);
            // If variable is not defined in scope, it might be undef
            symbol_table.find_symbol(name, enclosing_scope, SymbolKind::scalar()).is_empty()
        }
        NodeKind::Undef => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::parser::Parser;
    use perl_test_must::must_some_with;
    use perl_semantic_analyzer::analysis::symbol::SymbolExtractor;
    use perl_tdd_support::{must, must_some};

    fn common_mistakes_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let symbol_table = SymbolExtractor::new_with_source(source).extract(&ast);
        let mut diagnostics = Vec::new();
        check_common_mistakes(&ast, &symbol_table, &mut diagnostics);
        diagnostics
    }

    #[test]
    fn bareword_filehandle_open_is_flagged() {
        let diags = common_mistakes_diags(r#"open(FH, "<", "file.txt");"#);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL400")),
            "bareword filehandle should be flagged as PL400: {diags:?}"
        );
    }

    #[test]
    fn lexical_filehandle_open_is_not_flagged() {
        let diags = common_mistakes_diags(r#"open(my $fh, "<", "file.txt");"#);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL400")),
            "lexical filehandle open should not be flagged as PL400: {diags:?}"
        );
    }

    #[test]
    fn std_handles_are_not_flagged() {
        let diags = common_mistakes_diags(r#"open(STDOUT, ">", "out.txt");"#);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL400")),
            "STDOUT handle should not be flagged as PL400: {diags:?}"
        );
    }

    // --- PL403: assignment in condition ---

    #[test]
    fn assignment_in_if_condition_fires_pl403() {
        let diags = common_mistakes_diags("my $x; if ($x = 5) { }");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL403")),
            "assignment in `if` condition should fire PL403: {diags:?}"
        );
    }

    #[test]
    fn comparison_in_if_condition_no_pl403() {
        let diags = common_mistakes_diags("my $x = 0; if ($x == 5) { }");
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL403")),
            "comparison in `if` condition should not fire PL403: {diags:?}"
        );
    }

    #[test]
    fn assignment_in_while_condition_fires_pl403() {
        let diags = common_mistakes_diags("my $line; while ($line = 'data') { last; }");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL403")),
            "assignment in `while` condition should fire PL403: {diags:?}"
        );
    }

    #[test]
    fn string_comparison_in_if_no_pl403() {
        let diags = common_mistakes_diags(r#"my $s = ""; if ($s eq "hello") { }"#);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL403")),
            "string eq comparison should not fire PL403: {diags:?}"
        );
    }

    // --- PL404: numeric comparison with potentially undefined value ---

    #[test]
    fn numeric_compare_with_explicit_undef_fires_pl404() {
        let diags = common_mistakes_diags("if (undef == 5) { }");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL404")),
            "numeric comparison with `undef` should fire PL404: {diags:?}"
        );
    }

    #[test]
    fn numeric_compare_with_undef_not_equal_fires_pl404() {
        let diags = common_mistakes_diags("if (undef != 0) { }");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL404")),
            "!= comparison with `undef` should fire PL404: {diags:?}"
        );
    }

    #[test]
    fn numeric_compare_with_declared_scalar_no_pl404() {
        let diags = common_mistakes_diags("my $x = 10; if ($x == 5) { }");
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL404")),
            "numeric compare with declared scalar should not fire PL404: {diags:?}"
        );
    }

    #[test]
    fn numeric_compare_with_sub_local_scalar_no_pl404() {
        // Regression for #3644: `my $x` declared inside a subroutine body is
        // lexically visible for the rest of the enclosing block (perlsub,
        // "Persistent Private Variables" / perlsyn "my"). find_symbol used
        // to be called with a hard-coded global ScopeId (0), which meant a
        // `my` declared inside a sub (ScopeId > 0) was invisible to the
        // lookup and PL404 wrongly fired on a well-defined lexical.
        let diags = common_mistakes_diags("sub is_five { my $x = 10; if ($x == 5) { } }");
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL404")),
            "numeric compare with sub-local declared scalar should not fire PL404: {diags:?}"
        );
    }

    #[test]
    fn numeric_compare_with_nested_block_local_scalar_no_pl404() {
        // Regression for #3695 (re-applying #3644/#3659 hardening): `my $x`
        // declared inside a nested block (an `if` block inside a `sub`, not
        // just directly inside the sub body) must still be visible at the
        // comparison site. This exercises scope_at_offset picking the
        // innermost of two NESTED (not just one-level) enclosing scopes.
        let diags = common_mistakes_diags("sub outer { if (1) { my $x = 10; if ($x == 5) { } } }");
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL404")),
            "numeric compare with nested-block declared scalar should not fire PL404: {diags:?}"
        );
    }

    #[test]
    fn numeric_compare_with_sub_local_undeclared_scalar_fires_pl404() {
        // True-positive guard: a genuinely undeclared variable inside a sub
        // should still fire PL404 -- the fix must not over-suppress across
        // scope boundaries.
        let diags = common_mistakes_diags("sub is_five { if ($y == 5) { } }");
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL404")),
            "numeric compare with truly undeclared scalar inside a sub should still fire PL404: {diags:?}"
        );
    }

    #[test]
    fn pl403_diagnostic_suggests_double_equals() {
        let diags = common_mistakes_diags("my $x; if ($x = 5) { }");
        let pl403 = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL403")));
        let suggestion = pl403.suggestion.as_deref().unwrap_or("");
        assert!(
            suggestion.contains("==") || suggestion.contains("eq"),
            "PL403 suggestion should mention '==' or 'eq': {suggestion}"
        );
        assert!(
            pl403
                .related_information
                .iter()
                .all(|info| !info.message.contains('💡') && !info.message.contains('ℹ')),
            "PL403 related information should not use emoji: {:?}",
            pl403.related_information
        );
    }

    // --- producer-owned PL404 critic shapes (#11918) ---

    #[test]
    fn pl404_emitter_declares_the_shape_observed_at_the_branch() {
        use crate::tooling::perl_critic::CriticFindingShape;

        let literal = common_mistakes_diags("if (5 == undef) { }");
        let literal_observation = must_some_with(
            literal
                .iter()
                .find(|d| d.code.as_deref() == Some("PL404"))
                .and_then(|d| d.critic_observation.as_ref()),
            format!("literal undef PL404 must carry an observation: {literal:?}"),
        );
        assert_eq!(
            literal_observation.identity().shape(),
            CriticFindingShape::LiteralUndefComparison
        );

        let dataflow = common_mistakes_diags("if ($undeclared_var == 5) { }");
        let dataflow_observation = must_some_with(
            dataflow
                .iter()
                .find(|d| d.code.as_deref() == Some("PL404"))
                .and_then(|d| d.critic_observation.as_ref()),
            format!("data-flow PL404 must carry an observation: {dataflow:?}"),
        );
        assert_eq!(
            dataflow_observation.identity().shape(),
            CriticFindingShape::PotentiallyUndefComparison
        );
    }

    #[test]
    fn pl404_observations_declare_the_critic_scale_severity_the_producer_owns() {
        let diags = common_mistakes_diags("if (5 == undef) { }");
        let observation = must_some_with(
            diags
                .iter()
                .find(|d| d.code.as_deref() == Some("PL404"))
                .and_then(|d| d.critic_observation.as_ref()),
            format!("PL404 must carry an observation: {diags:?}"),
        );
        // Stern matches the reviewed native alias declaration; deriving it
        // from the LSP Warning instead would be an invented mapping.
        assert_eq!(observation.severity(), crate::tooling::perl_critic::Severity::Stern);
    }

    /// #12004: the observation's remediation copy must stay identical to the
    /// ordinary diagnostic fields it mirrors, or merged rows silently serve
    /// stale text after the ordinary row retires.
    #[test]
    fn pl404_observation_remediation_copies_match_the_ordinary_diagnostic_fields() {
        for source in ["if (5 == undef) { }", "if ($undeclared_var == 5) { }"] {
            let diags = common_mistakes_diags(source);
            let diagnostic = must_some_with(
                diags.iter().find(|d| d.code.as_deref() == Some("PL404")),
                format!("PL404 must be emitted for {source}"),
            );
            let suggestion = must_some_with(
                diagnostic.suggestion.as_deref(),
                "PL404 must carry an ordinary suggestion",
            );
            let observation = must_some_with(
                diagnostic.critic_observation.as_ref(),
                format!("PL404 must carry an observation: {diags:?}"),
            );

            assert_eq!(
                observation.suggestion(),
                Some(suggestion),
                "PL404: observation suggestion drifted from the ordinary diagnostic"
            );
            let ordinary_related = diagnostic
                .related_information
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>();
            let observation_related = observation
                .related_information()
                .iter()
                .map(|(_, m)| m.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                observation_related, ordinary_related,
                "PL404: observation related information drifted from the ordinary diagnostic"
            );
        }
    }

    #[test]
    fn non_overlap_common_mistakes_carry_no_observation() {
        let diags = common_mistakes_diags("my $x; if ($x = 5) { }");
        assert!(
            diags.iter().all(|d| d.critic_observation.is_none()),
            "PL403 is outside the reviewed overlap cohort: {diags:?}"
        );
    }
}
