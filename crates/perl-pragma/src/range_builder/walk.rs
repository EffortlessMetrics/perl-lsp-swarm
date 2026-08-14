use super::directives::{apply_no_directive, apply_use_directive};
use crate::{PragmaState, enable_effective_version_semantics, parse_perl_version};
use perl_ast::ast::{Node, NodeKind};
use std::ops::Range;

pub(crate) fn build_ranges(
    node: &Node,
    current_state: &mut PragmaState,
    ranges: &mut Vec<(Range<usize>, PragmaState)>,
) {
    match &node.kind {
        NodeKind::Use { module, args, .. } => {
            apply_use_directive(
                node.location.start..node.location.end,
                module,
                args,
                current_state,
                ranges,
            );
        }
        NodeKind::No { module, args, .. } => {
            apply_no_directive(
                node.location.start..node.location.end,
                module,
                args,
                current_state,
                ranges,
            );
        }
        NodeKind::Block { statements } => {
            build_statement_block(statements, node.location.end, current_state, ranges);
        }
        NodeKind::Program { statements } => {
            for stmt in statements {
                build_ranges(stmt, current_state, ranges);
            }
        }
        NodeKind::Subroutine { body, .. }
        | NodeKind::Method { body, .. }
        | NodeKind::Class { body, .. } => {
            build_scoped_body(body, current_state, ranges);
        }
        NodeKind::If { then_branch, elsif_branches, else_branch, .. } => {
            build_scoped_body(then_branch, current_state, ranges);
            for (_, elsif_body) in elsif_branches {
                build_scoped_body(elsif_body, current_state, ranges);
            }
            if let Some(else_b) = else_branch {
                build_scoped_body(else_b, current_state, ranges);
            }
        }
        NodeKind::While { body, continue_block, .. }
        | NodeKind::For { body, continue_block, .. }
        | NodeKind::Foreach { body, continue_block, .. } => {
            build_scoped_body(body, current_state, ranges);
            if let Some(continue_block) = continue_block {
                build_scoped_body(continue_block, current_state, ranges);
            }
        }
        NodeKind::Eval { block } => {
            if matches!(block.kind, NodeKind::Block { .. }) {
                build_scoped_body(block, current_state, ranges);
            }
        }
        NodeKind::Do { block } | NodeKind::Defer { block } | NodeKind::PhaseBlock { block, .. } => {
            build_scoped_body(block, current_state, ranges);
        }
        NodeKind::Given { body, .. } | NodeKind::When { body, .. } | NodeKind::Default { body } => {
            build_scoped_body(body, current_state, ranges);
        }
        NodeKind::Try { body, catch_blocks, finally_block } => {
            build_scoped_body(body, current_state, ranges);
            for (_, catch_body) in catch_blocks {
                build_scoped_body(catch_body, current_state, ranges);
            }
            if let Some(finally_block) = finally_block {
                build_scoped_body(finally_block, current_state, ranges);
            }
        }
        NodeKind::LabeledStatement { statement, .. } => {
            build_ranges(statement, current_state, ranges);
        }
        NodeKind::StatementModifier { statement, condition, .. } => {
            build_ranges(statement, current_state, ranges);
            build_ranges(condition, current_state, ranges);
        }
        NodeKind::Package { block: Some(pkg_block), .. } => {
            build_scoped_body(pkg_block, current_state, ranges);
        }
        // Handle `require VERSION` — in Perl, this enables the version's
        // feature bundle and strict/warnings lexically, just like `use VERSION`.
        // The parser produces FunctionCall { name: "require", args: [version] }.
        // (#5106)
        NodeKind::FunctionCall { name, args } if name == "require" => {
            if let Some(version_str) = extract_require_version(args)
                && let Some(version) = parse_perl_version(&version_str)
            {
                enable_effective_version_semantics(current_state, version);
                ranges.push((node.location.start..node.location.end, current_state.clone()));
            }
        }
        NodeKind::ExpressionStatement { expression } => {
            build_ranges(expression, current_state, ranges);
        }
        _ => {}
    }
}

/// Extract the version string from a `require VERSION` call's arguments.
/// Handles Number nodes (e.g. `require 5.036`), String nodes (e.g.
/// `require "v5.36"`), and bareword-identifier nodes (e.g. `require v5.36`).
fn extract_require_version(args: &[Node]) -> Option<String> {
    let first = args.first()?;
    match &first.kind {
        NodeKind::Number { value } => Some(value.clone()),
        NodeKind::String { value, .. } => Some(value.clone()),
        NodeKind::VString { value } => Some(value.clone()),
        NodeKind::Identifier { name } => Some(name.clone()),
        _ => None,
    }
}

fn build_scoped_body(
    body: &Node,
    current_state: &mut PragmaState,
    ranges: &mut Vec<(Range<usize>, PragmaState)>,
) {
    let saved_state = current_state.clone();
    build_ranges(body, current_state, ranges);
    if *current_state != saved_state {
        ranges.push((body.location.end..body.location.end, saved_state.clone()));
    }
    *current_state = saved_state;
}

fn build_statement_block(
    statements: &[Node],
    end: usize,
    current_state: &mut PragmaState,
    ranges: &mut Vec<(Range<usize>, PragmaState)>,
) {
    let saved_state = current_state.clone();
    for stmt in statements {
        build_ranges(stmt, current_state, ranges);
    }
    if *current_state != saved_state {
        ranges.push((end..end, saved_state.clone()));
    }
    *current_state = saved_state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_ast::SourceLocation;

    #[test]
    fn changed_scoped_body_emits_restore_entry_and_restores_state() {
        let saved_state = PragmaState::default();
        let mut current_state = saved_state.clone();
        let body = Node::new(
            NodeKind::Use {
                module: "strict".to_string(),
                args: Vec::new(),
                has_filter_risk: false,
            },
            SourceLocation { start: 10, end: 42 },
        );
        let mut ranges = Vec::new();

        build_scoped_body(&body, &mut current_state, &mut ranges);

        assert_eq!(
            ranges.len(),
            2,
            "changed scoped body should emit its directive and restore entries",
        );
        assert_eq!(
            ranges.last().map(|(range, state)| (range.clone(), state.clone())),
            Some((42..42, saved_state.clone())),
            "restore entry should be zero-length at the body end and hold the saved state",
        );
        assert_eq!(
            current_state, saved_state,
            "scoped body should restore the caller state after building its ranges",
        );
    }
}
