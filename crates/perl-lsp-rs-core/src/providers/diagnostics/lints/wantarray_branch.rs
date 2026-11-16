//! Irrelevant `wantarray()` branch detection (PL411).
//!
//! First conservative slice for issue #2993: warn when a subroutine or `eval`
//! block contains `if (wantarray()) { … } else { … }` whose arms contain no
//! executable statements (comments-only or empty blocks). The caller-context
//! query cannot change local behavior in that shape.
//!
//! Non-empty arms, `elsif`, `unless`, top-level code, and conditions other
//! than a bare `wantarray` / `CORE::wantarray` call remain out of scope.

use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::{Node, NodeKind};

use super::super::internal_types::{Diagnostic, RelatedInformation};
use super::super::walker::walk_node;

/// Warn on `wantarray()` conditionals whose branches perform no local work.
pub fn check_irrelevant_wantarray_branches(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    walk_node(root, &mut |node| match &node.kind {
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            scan_execution_unit(body, diagnostics);
        }
        NodeKind::Eval { block, .. } => {
            scan_execution_unit(block, diagnostics);
        }
        _ => {}
    });
}

fn scan_execution_unit(root: &Node, diagnostics: &mut Vec<Diagnostic>) {
    walk_execution_unit(root, &mut |node| {
        let NodeKind::If { condition, then_branch, elsif_branches, else_branch, keyword } =
            &node.kind
        else {
            return;
        };

        if keyword.as_deref() == Some("unless") {
            return;
        }
        if !elsif_branches.is_empty() {
            return;
        }
        let Some(else_branch) = else_branch else {
            return;
        };
        if !is_bare_wantarray_condition(condition) {
            return;
        }
        if !branch_has_no_executable_statements(then_branch)
            || !branch_has_no_executable_statements(else_branch)
        {
            return;
        }

        let range = (node.location.start, node.location.end);
        diagnostics.push(Diagnostic {
            range,
            severity: DiagnosticSeverity::Warning,
            code: Some(DiagnosticCode::IrrelevantWantarrayBranch.as_str().to_string()),
            message: "`wantarray()` chooses between branches that perform no work; \
                      the caller-context check has no effect here"
                .to_string(),
            related_information: vec![RelatedInformation {
                location: range,
                message: "Remove the conditional or add distinct behavior in each branch \
                          (for example context-specific returns)."
                    .to_string(),
            }],
            tags: Vec::new(),
            fixable: false,
            critic_observation: None,
            suggestion: Some(
                "Drop the `if (wantarray())` when both branches are empty, or return \
                 different values per context."
                    .to_string(),
            ),
        });
    });
}

fn walk_execution_unit(node: &Node, visitor: &mut impl FnMut(&Node)) {
    if matches!(
        &node.kind,
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } | NodeKind::Eval { .. }
    ) {
        return;
    }

    visitor(node);
    for child in node.children() {
        walk_execution_unit(child, visitor);
    }
}

fn is_bare_wantarray_condition(node: &Node) -> bool {
    match &node.kind {
        NodeKind::FunctionCall { name, args } if args.is_empty() => {
            matches!(name.as_str(), "wantarray" | "CORE::wantarray")
        }
        _ => false,
    }
}

fn branch_has_no_executable_statements(branch: &Node) -> bool {
    match &branch.kind {
        NodeKind::Block { statements } => !statements.iter().any(is_executable_statement),
        _ => !is_executable_statement(branch),
    }
}

fn is_executable_statement(stmt: &Node) -> bool {
    match &stmt.kind {
        NodeKind::Block { statements } => statements.iter().any(is_executable_statement),
        // Any expression statement can affect implicit return or side effects; only
        // comment-only / empty blocks are "no work" for this conservative slice.
        NodeKind::ExpressionStatement { .. } => true,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_tdd_support::must_some;

    fn diags_for(source: &str) -> Vec<Diagnostic> {
        let output = Parser::new(source).parse_with_recovery();
        let mut diags = Vec::new();
        check_irrelevant_wantarray_branches(&output.ast, &mut diags);
        diags
    }

    fn pl411(source: &str) -> Vec<Diagnostic> {
        diags_for(source).into_iter().filter(|d| d.code.as_deref() == Some("PL411")).collect()
    }

    #[test]
    fn empty_wantarray_branches_in_sub_warns() {
        let source = r#"use v5.40;
sub get_data {
    if (wantarray()) {
        # list context
    } else {
        # scalar context
    }
    return 42;
}
"#;
        assert_eq!(pl411(source).len(), 1, "expected PL411 for empty arms");
    }

    #[test]
    fn distinct_returns_do_not_warn() {
        let source = r#"use v5.40;
sub get_data {
    if (wantarray()) {
        return (1, 2, 3);
    } else {
        return "scalar";
    }
}
"#;
        assert!(pl411(source).is_empty(), "distinct branch returns must not warn");
    }

    #[test]
    fn empty_arms_warn_even_with_shared_following_return() {
        let source = r#"use v5.40;
sub get_data {
    if (wantarray()) {
    } else {
    }
    return 42;
}
"#;
        assert_eq!(
            pl411(source).len(),
            1,
            "empty arms remain irrelevant even when a shared return follows"
        );
    }

    #[test]
    fn ternary_wantarray_does_not_warn() {
        let source = r#"use v5.40;
sub ternary_context {
    return wantarray ? (1, 2) : 42;
}
"#;
        assert!(pl411(source).is_empty());
    }

    #[test]
    fn defined_wantarray_does_not_warn() {
        let source = r#"use v5.40;
sub ctx {
    if (defined wantarray()) {
    } else {
    }
}
"#;
        assert!(pl411(source).is_empty());
    }

    #[test]
    fn top_level_if_wantarray_does_not_warn() {
        let source = r#"use v5.40;
if (wantarray()) {
} else {
}
"#;
        assert!(pl411(source).is_empty(), "top-level wantarray is out of scope");
    }

    #[test]
    fn undef_in_branch_is_context_sensitive_return() {
        let source = r#"use v5.40;
sub f {
    if (wantarray()) {
        undef;
    } else {
    }
}
"#;
        assert!(pl411(source).is_empty(), "undef in a branch is observable return work");
    }

    #[test]
    fn implicit_literal_returns_do_not_warn() {
        let source = r#"use v5.40;
sub f {
    if (wantarray()) {
        1;
    } else {
        2;
    }
}
"#;
        assert!(pl411(source).is_empty(), "implicit if return values differ by context");
    }

    #[test]
    fn diagnostic_targets_if_statement() {
        let source = r#"use v5.40;
sub s {
    if (wantarray()) {
    } else {
    }
}
"#;
        let diags = pl411(source);
        let diag = must_some(diags.first());
        assert!(
            diag.message.contains("wantarray"),
            "message should mention wantarray: {}",
            diag.message
        );
        assert_eq!(diag.code.as_deref(), Some("PL411"));
    }
}
