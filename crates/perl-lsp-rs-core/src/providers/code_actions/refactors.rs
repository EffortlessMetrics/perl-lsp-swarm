//! Refactoring actions for code transformations
//!
//! Provides automated refactoring operations for improving code structure.

use super::enhanced::{detect_parameters, is_admissible_standalone_extract_block};
use super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser::ast_utils::{
    find_function_insert_position, find_node_at_range, find_statement_start,
};
use perl_parser_core::{Node, NodeKind, SourceLocation};

/// Get refactoring actions for a selection
pub fn get_refactoring_actions(source: &str, ast: &Node, range: (usize, usize)) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // Use the enhanced provider for better refactorings
    let enhanced_provider = super::enhanced::EnhancedCodeActionsProvider::new(source.to_string());
    actions.extend(enhanced_provider.get_enhanced_refactoring_actions(ast, range));

    // Keep basic extract-variable as a fallback. Basic extract-function is
    // withheld (#16779): it copies a selected block into a parameterless
    // subroutine and replaces the original braces with a call, so a surrounding
    // `my` capture is dropped and `if`/`else` bodies can lose their braces.
    // Prefer the enhanced extract-subroutine path, which can prove a calling
    // convention, or return no extract-function edit.
    if let Some(node) = find_node_at_range(ast, range)
        && matches!(&node.kind, NodeKind::Block { .. })
        && !actions.iter().any(|action| action.title == "Extract to subroutine")
        && let Some(action) = standalone_block_extract_function(source, ast, node)
    {
        actions.push(action);
    }

    if actions.is_empty()
        && let Some(node) = find_node_at_range(ast, range)
        && matches!(&node.kind, NodeKind::FunctionCall { .. } | NodeKind::Binary { .. })
    {
        actions.push(CodeAction {
            title: "Extract to variable".to_string(),
            kind: CodeActionKind::RefactorExtractVariable,
            diagnostics: Vec::new(),
            edit: extract_variable(source, node, range),
            is_preferred: false,
        });
    }

    actions
}

/// #16784: narrow standalone top-level block cohort for basic extract-function.
///
/// Copies the historical edit shape only when the block is not inside a
/// subroutine or control-flow body, carries no outer lexical captures, and
/// matches the same admissible-statement grammar as enhanced extraction.
fn standalone_block_extract_function(source: &str, ast: &Node, block: &Node) -> Option<CodeAction> {
    if !matches!(&block.kind, NodeKind::Block { .. }) {
        return None;
    }
    if block_is_inside_subroutine(ast, block) || block_is_control_flow_body(ast, block) {
        return None;
    }
    if !is_admissible_standalone_extract_block(block) {
        return None;
    }
    let params = detect_parameters(block)?;
    if !params.is_empty() {
        return None;
    }
    if !standalone_block_preamble_is_safe(source, block.location.start) {
        return None;
    }
    if !standalone_block_suffix_is_safe(source, block.location.end) {
        return None;
    }
    if block_uses_caller_call(block) {
        return None;
    }
    if source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("__DATA__") || line.starts_with("__END__")
    }) {
        return None;
    }
    let func_name = suggest_extract_function_name(source)?;
    let body_text = source.get(block.location.start..block.location.end)?;
    let insert_pos = find_function_insert_position(source);
    Some(CodeAction {
        title: "Extract to function".to_string(),
        kind: CodeActionKind::RefactorExtract,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![
                TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: format!("\nsub {func_name} {{\n{body_text}\n}}\n"),
                },
                TextEdit { location: block.location, new_text: format!("{func_name}();") },
            ],
        },
        is_preferred: false,
    })
}

fn suggest_extract_function_name(source: &str) -> Option<String> {
    for suffix in 1..=1000 {
        let name = if suffix == 1 {
            "extracted_function".to_string()
        } else {
            format!("extracted_function_{suffix}")
        };
        if !source.match_indices(&name).any(|(start, _)| {
            let before = source[..start].chars().next_back();
            let after = source[start + name.len()..].chars().next();
            before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                && after.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
        }) {
            return Some(name);
        }
    }
    None
}

fn standalone_block_preamble_is_safe(source: &str, block_start: usize) -> bool {
    let Some(preamble) = source.get(..block_start) else {
        return false;
    };
    if !preamble.lines().any(|line| line.trim() == "use strict;") {
        return false;
    }
    for line in preamble.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "use strict;" || trimmed == "use warnings;" {
            continue;
        }
        if trimmed.starts_with("use feature") || trimmed.starts_with("use open") {
            continue;
        }
        if trimmed.starts_with("use ") || trimmed.starts_with("no ") {
            return false;
        }
        if trimmed.starts_with("package ")
            || trimmed.starts_with("our ")
            || trimmed.starts_with("state ")
            || trimmed.starts_with("local ")
        {
            return false;
        }
    }
    true
}

/// EOF insertion must not cross a later `package` boundary or lexical pragma
/// change that would not apply to the extracted subroutine body.
fn standalone_block_suffix_is_safe(source: &str, block_end: usize) -> bool {
    let Some(suffix) = source.get(block_end..) else {
        return false;
    };
    for line in suffix.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("package ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("no ")
            || trimmed.starts_with("our ")
            || trimmed.starts_with("state ")
            || trimmed.starts_with("local ")
        {
            return false;
        }
    }
    true
}

fn block_uses_caller_call(node: &Node) -> bool {
    let mut found = false;
    visit_for_caller_call(node, &mut found);
    found
}

fn visit_for_caller_call(node: &Node, found: &mut bool) {
    if *found {
        return;
    }
    if matches!(&node.kind, NodeKind::FunctionCall { name, .. } if name == "caller") {
        *found = true;
        return;
    }
    for child in node.children() {
        visit_for_caller_call(child, found);
    }
}

fn block_is_inside_subroutine(ast: &Node, target: &Node) -> bool {
    let mut inside = false;
    visit_subroutine_enclosure(ast, target, false, &mut inside);
    inside
}

fn block_is_control_flow_body(ast: &Node, target: &Node) -> bool {
    let mut control_body = false;
    visit_control_flow_body(ast, target, false, &mut control_body);
    control_body
}

fn visit_subroutine_enclosure(node: &Node, target: &Node, in_subroutine: bool, matched: &mut bool) {
    if std::ptr::eq(node, target) && in_subroutine {
        *matched = true;
        return;
    }
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for stmt in statements {
                visit_subroutine_enclosure(stmt, target, in_subroutine, matched);
            }
        }
        NodeKind::Subroutine { body, .. } => {
            visit_subroutine_enclosure(body, target, true, matched);
        }
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            visit_subroutine_enclosure(condition, target, in_subroutine, matched);
            visit_subroutine_enclosure(then_branch, target, in_subroutine, matched);
            for (cond, branch) in elsif_branches {
                visit_subroutine_enclosure(cond, target, in_subroutine, matched);
                visit_subroutine_enclosure(branch, target, in_subroutine, matched);
            }
            if let Some(branch) = else_branch {
                visit_subroutine_enclosure(branch, target, in_subroutine, matched);
            }
        }
        NodeKind::While { condition, body, .. } => {
            visit_subroutine_enclosure(condition, target, in_subroutine, matched);
            visit_subroutine_enclosure(body, target, in_subroutine, matched);
        }
        NodeKind::For { init, condition, update, body, .. } => {
            if let Some(init) = init {
                visit_subroutine_enclosure(init, target, in_subroutine, matched);
            }
            if let Some(condition) = condition {
                visit_subroutine_enclosure(condition, target, in_subroutine, matched);
            }
            if let Some(update) = update {
                visit_subroutine_enclosure(update, target, in_subroutine, matched);
            }
            visit_subroutine_enclosure(body, target, in_subroutine, matched);
        }
        NodeKind::Foreach { variable, list, body, .. } => {
            visit_subroutine_enclosure(variable, target, in_subroutine, matched);
            visit_subroutine_enclosure(list, target, in_subroutine, matched);
            visit_subroutine_enclosure(body, target, in_subroutine, matched);
        }
        NodeKind::ExpressionStatement { expression } => {
            visit_subroutine_enclosure(expression, target, in_subroutine, matched);
        }
        NodeKind::Binary { left, right, .. } => {
            visit_subroutine_enclosure(left, target, in_subroutine, matched);
            visit_subroutine_enclosure(right, target, in_subroutine, matched);
        }
        _ => {
            for child in node.children() {
                visit_subroutine_enclosure(child, target, in_subroutine, matched);
            }
        }
    }
}

fn visit_control_flow_body(node: &Node, target: &Node, is_control_body: bool, matched: &mut bool) {
    if std::ptr::eq(node, target) && is_control_body {
        *matched = true;
        return;
    }
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for stmt in statements {
                visit_control_flow_body(stmt, target, is_control_body, matched);
            }
        }
        NodeKind::Subroutine { body, .. } => {
            visit_control_flow_body(body, target, true, matched);
        }
        NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
            visit_control_flow_body(condition, target, false, matched);
            visit_control_flow_body(then_branch, target, true, matched);
            for (cond, branch) in elsif_branches {
                visit_control_flow_body(cond, target, false, matched);
                visit_control_flow_body(branch, target, true, matched);
            }
            if let Some(branch) = else_branch {
                visit_control_flow_body(branch, target, true, matched);
            }
        }
        NodeKind::While { condition, body, .. } => {
            visit_control_flow_body(condition, target, false, matched);
            visit_control_flow_body(body, target, true, matched);
        }
        NodeKind::For { init, condition, update, body, .. } => {
            if let Some(init) = init {
                visit_control_flow_body(init, target, false, matched);
            }
            if let Some(condition) = condition {
                visit_control_flow_body(condition, target, false, matched);
            }
            if let Some(update) = update {
                visit_control_flow_body(update, target, false, matched);
            }
            visit_control_flow_body(body, target, true, matched);
        }
        NodeKind::Foreach { variable, list, body, .. } => {
            visit_control_flow_body(variable, target, false, matched);
            visit_control_flow_body(list, target, false, matched);
            visit_control_flow_body(body, target, true, matched);
        }
        NodeKind::ExpressionStatement { expression } => {
            visit_control_flow_body(expression, target, is_control_body, matched);
        }
        NodeKind::Binary { left, right, .. } => {
            visit_control_flow_body(left, target, is_control_body, matched);
            visit_control_flow_body(right, target, is_control_body, matched);
        }
        _ => {
            for child in node.children() {
                visit_control_flow_body(child, target, is_control_body, matched);
            }
        }
    }
}

/// Extract expression to variable
fn extract_variable(source: &str, node: &Node, _range: (usize, usize)) -> CodeActionEdit {
    let expr_text = &source[node.location.start..node.location.end];
    let var_name = "$extracted_var";

    // Find statement start
    let stmt_start = find_statement_start(source, node.location.start);

    CodeActionEdit {
        changes: vec![
            // Insert variable declaration
            TextEdit {
                location: SourceLocation { start: stmt_start, end: stmt_start },
                new_text: format!("my {} = {};\n", var_name, expr_text),
            },
            // Replace expression with variable
            TextEdit { location: node.location, new_text: var_name.to_string() },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::{must, must_some_with};
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Reproduced from #16779: a selected `if` body inside a standalone outer
    /// block. Enhanced extract-subroutine declines this control-flow body; before
    /// #16779, the basic fallback was the only extract-function producer.
    const CAPTURE_FIXTURE: &str = "use strict;\nuse warnings;\n{\n    my $x = 1;\n    if (1) { warn $x; } else { warn 0; }\n}\n";
    const CAPTURE_BLOCK: &str = "{ warn $x; }";

    const NONCAPTURE_FIXTURE: &str =
        "use strict;\nuse warnings;\n{\n    if (1) { warn 0; } else { warn 1; }\n}\n";
    const NONCAPTURE_BLOCK: &str = "{ warn 0; }";

    const ARRAY_CAPTURE_FIXTURE: &str = "use strict;\nuse warnings;\n{\n    my @items = (1, 2);\n    if (1) { my $n = @items; } else { warn 0; }\n}\n";
    const ARRAY_CAPTURE_BLOCK: &str = "{ my $n = @items; }";

    const SAFE_EXTRACT_FIXTURE: &str = "use strict;\n\
                      use warnings;\n\
                      sub worker {\n\
                      \x20   my $base = 10;\n\
                      \x20   {\n\
                      \x20       my $x = $base * 2;\n\
                      \x20       $x + 1;\n\
                      \x20   }\n\
                      }\n\
                      print worker(), \"\\n\";\n";

    const STANDALONE_EXTRACT_FIXTURE: &str =
        "use strict;\nuse warnings;\nuse feature \"say\";\n{ say \"ok\"; }\n";
    const STANDALONE_BLOCK: &str = "{ say \"ok\"; }";

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        must(parser.parse())
    }

    fn byte_range(source: &str, needle: &str) -> (usize, usize) {
        let start = must_some_with(source.find(needle), "fixture must contain the selected block");
        (start, start + needle.len())
    }

    fn refactor_actions(source: &str, needle: &str) -> Vec<CodeAction> {
        let ast = parse(source);
        get_refactoring_actions(source, &ast, byte_range(source, needle))
    }

    fn extract_function_actions(actions: &[CodeAction]) -> Vec<&CodeAction> {
        actions.iter().filter(|action| action.title == "Extract to function").collect()
    }

    fn apply_action(source: &str, action: &CodeAction) -> String {
        let mut changes: Vec<&TextEdit> = action.edit.changes.iter().collect();
        changes.sort_by_key(|change| std::cmp::Reverse(change.location.start));
        let mut edited = source.to_string();
        for change in changes {
            edited.replace_range(change.location.start..change.location.end, &change.new_text);
        }
        edited
    }

    fn perl_stdout(source: &str) -> String {
        let mut child = must(
            Command::new("perl")
                .arg("-")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        );
        if let Some(mut stdin) = child.stdin.take() {
            must(stdin.write_all(source.as_bytes()));
        }
        let output = must(child.wait_with_output());
        assert!(
            output.status.success(),
            "Perl execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn perl_compiles(source: &str) -> bool {
        let mut child = must(
            Command::new("perl")
                .args(["-c", "-"])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        );
        if let Some(mut stdin) = child.stdin.take() {
            must(stdin.write_all(source.as_bytes()));
        }
        must(child.wait_with_output()).status.success()
    }

    fn perl_stderr(source: &str) -> String {
        let mut child = must(
            Command::new("perl")
                .args(["-c", "-"])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
        );
        if let Some(mut stdin) = child.stdin.take() {
            must(stdin.write_all(source.as_bytes()));
        }
        let output = must(child.wait_with_output());
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// Reconstruct the historical basic fallback edit: copy the selected
    /// braces into a parameterless subroutine at EOF and replace them with a
    /// call. Offsets are applied high-to-low so the original range stays valid.
    fn apply_historical_basic_extract(source: &str, block: &str) -> String {
        let (start, end) = byte_range(source, block);
        let insert = format!("\nsub extracted_function {{\n{block}\n}}\n");
        let mut changes = vec![
            (source.len(), source.len(), insert),
            (start, end, "extracted_function();".to_string()),
        ];
        changes.sort_by_key(|change| std::cmp::Reverse(change.0));
        let mut edited = source.to_string();
        for (from, to, text) in changes {
            edited.replace_range(from..to, &text);
        }
        edited
    }

    /// #16779: selecting `{ warn $x; }` must not offer a parameterless
    /// extract-function edit that moves `$x` out of its `my` scope.
    #[test]
    fn capturing_if_body_does_not_offer_basic_extract_function() {
        assert!(perl_compiles(CAPTURE_FIXTURE), "fixture itself must compile");
        let actions = refactor_actions(CAPTURE_FIXTURE, CAPTURE_BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "basic extract-function must not be offered for a capturing block, got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
        assert!(
            !actions.iter().any(|action| {
                action.edit.changes.iter().any(|edit| {
                    edit.new_text.contains("sub extracted_function")
                        && edit.new_text.contains("warn $x")
                        && !edit.new_text.contains("my ($x)")
                })
            }),
            "no parameterless extracted_function edit may capture outer $x, got: {:?}",
            actions
        );
        let (block_start, block_end) = byte_range(CAPTURE_FIXTURE, CAPTURE_BLOCK);
        assert!(
            !actions.iter().any(|action| {
                action.edit.changes.iter().any(|edit| {
                    edit.location.start == block_start
                        && edit.location.end == block_end
                        && edit.new_text.contains("();")
                        && !edit.new_text.contains("$x")
                })
            }),
            "no replacement of the capturing block by a parameterless call, got: {:?}",
            actions
        );
    }

    /// The same fallback also drops the `if` body's braces. Fail closed when
    /// enhanced extraction declined, even if the body does not capture.
    #[test]
    fn declined_noncapturing_if_body_does_not_offer_basic_extract_function() {
        assert!(perl_compiles(NONCAPTURE_FIXTURE), "fixture itself must compile");
        let actions = refactor_actions(NONCAPTURE_FIXTURE, NONCAPTURE_BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "basic extract-function must not run after enhanced extraction declined, got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }

    /// An array capture cannot travel through a scalar `@_` convention, so
    /// capture safety cannot be established. The basic fallback must not guess.
    #[test]
    fn unproven_array_capture_does_not_offer_basic_extract_function() {
        let actions = refactor_actions(ARRAY_CAPTURE_FIXTURE, ARRAY_CAPTURE_BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "basic extract-function must not be offered when capture safety is unproven, got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }

    /// Independent Perl oracle for the edit the basic fallback used to return.
    /// Brace loss is the first compile failure; repairing only the `if` braces
    /// still leaves `$x` out of scope under `use strict`.
    #[test]
    fn historical_basic_extract_of_capturing_if_body_fails_perl_c() {
        assert!(perl_compiles(CAPTURE_FIXTURE), "source fixture must compile before the bad edit");
        let edited = apply_historical_basic_extract(CAPTURE_FIXTURE, CAPTURE_BLOCK);
        assert!(!perl_compiles(&edited), "historical extract must fail perl -c:\n{edited}");
        let stderr = perl_stderr(&edited);
        assert!(
            stderr.contains("syntax error") || stderr.contains("Bareword found"),
            "historical extract must fail from brace-losing replacement, got:\n{stderr}\n{edited}"
        );
        let brace_repaired = edited
            .replace("if (1) extracted_function(); else", "if (1) { extracted_function(); } else");
        assert!(
            !perl_compiles(&brace_repaired),
            "brace-only repair must still fail perl -c:\n{brace_repaired}"
        );
        let repaired_stderr = perl_stderr(&brace_repaired);
        assert!(
            repaired_stderr.contains("Global symbol \"$x\" requires explicit package name"),
            "brace-only repair must still leave $x out of scope, got:\n{repaired_stderr}\n{brace_repaired}"
        );
    }

    /// Retention control: a standalone block the enhanced generator can prove
    /// still yields extract-to-subroutine, with typed extract kind unchanged.
    #[test]
    fn proven_enhanced_extract_subroutine_is_still_offered() {
        let ast = parse(SAFE_EXTRACT_FIXTURE);
        let block_start = must_some_with(
            SAFE_EXTRACT_FIXTURE.find("{\n        my $x = $base * 2;"),
            "safe fixture must contain the extractable block",
        );
        let block_end = must_some_with(
            SAFE_EXTRACT_FIXTURE[block_start..].find("\n    }"),
            "safe fixture must contain the extractable block end",
        );
        let range = (block_start, block_start + block_end + "\n    }".len());
        let actions = get_refactoring_actions(SAFE_EXTRACT_FIXTURE, &ast, range);
        let extract = actions.iter().find(|action| action.title == "Extract to subroutine");
        let extract = must_some_with(extract, "enhanced extract-subroutine must remain available");
        assert_eq!(extract.kind, CodeActionKind::RefactorExtractSubroutine);
        assert!(
            extract.edit.changes.iter().any(|edit| edit.new_text.contains("my ($base) = @_;")),
            "enhanced path must still carry the captured lexical:\n{:?}",
            extract.edit.changes
        );
        assert!(
            extract_function_actions(&actions).is_empty(),
            "basic extract-function must not shadow the proven enhanced path"
        );
    }

    /// #16784: the admitted standalone cohort restores the historical edit only
    /// for a noncapturing top-level block with a shared strict/warnings preamble.
    #[test]
    fn standalone_noncapturing_block_offers_extract_function() {
        assert!(perl_compiles(STANDALONE_EXTRACT_FIXTURE), "fixture must compile");
        let actions = refactor_actions(STANDALONE_EXTRACT_FIXTURE, STANDALONE_BLOCK);
        let extract = must_some_with(
            extract_function_actions(&actions).first().copied(),
            "admitted standalone block must offer Extract to function",
        );
        assert_eq!(extract.kind, CodeActionKind::RefactorExtract);
        let edited = apply_action(STANDALONE_EXTRACT_FIXTURE, extract);
        assert!(perl_compiles(&edited), "applied edit must pass perl -c:\n{edited}");
        assert_eq!(
            perl_stdout(STANDALONE_EXTRACT_FIXTURE).trim(),
            perl_stdout(&edited).trim(),
            "applied edit must preserve runtime output"
        );
        assert!(
            edited.contains("sub extracted_function"),
            "edit must insert the extracted subroutine:\n{edited}"
        );
    }

    /// The historical parameterless edit shape is safe only on the admitted
    /// cohort; the same shape on a capturing if-body still fails `perl -c`.
    #[test]
    fn historical_basic_extract_on_admitted_standalone_passes_perl_c() {
        let edited = apply_historical_basic_extract(STANDALONE_EXTRACT_FIXTURE, STANDALONE_BLOCK);
        assert!(perl_compiles(&edited), "admitted historical edit must compile:\n{edited}");
    }

    /// EOF insertion must not place the new sub after a trailing `package` line.
    #[test]
    fn standalone_block_with_trailing_package_does_not_offer_extract_function() {
        const FIXTURE: &str =
            "use strict;\nuse warnings;\nuse feature \"say\";\n{ say \"ok\"; }\npackage Other;\n";
        const BLOCK: &str = "{ say \"ok\"; }";
        assert!(perl_compiles(FIXTURE), "fixture must compile");
        let actions = refactor_actions(FIXTURE, BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "must not extract when a package boundary follows the block, got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }

    /// Lexical pragma changes after the block would not apply to an EOF sub.
    #[test]
    fn standalone_block_with_trailing_no_feature_does_not_offer_extract_function() {
        const FIXTURE: &str = "use strict;\nuse warnings;\nuse feature \"say\";\n{ say \"ok\"; }\nno feature \"say\";\n";
        const BLOCK: &str = "{ say \"ok\"; }";
        assert!(perl_compiles(FIXTURE), "fixture must compile");
        let actions = refactor_actions(FIXTURE, BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "must not extract when trailing no feature follows the block, got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }

    /// `caller()` stack depth changes when the block moves into a subroutine.
    #[test]
    fn standalone_block_using_caller_does_not_offer_extract_function() {
        const FIXTURE: &str = "use strict;\nuse warnings;\n{ my @c = caller(); }\n";
        const BLOCK: &str = "{ my @c = caller(); }";
        assert!(perl_compiles(FIXTURE), "fixture must compile");
        let actions = refactor_actions(FIXTURE, BLOCK);
        assert!(
            extract_function_actions(&actions).is_empty(),
            "must not extract when the block calls caller(), got: {:?}",
            actions.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }

    /// After one extraction, repeat requests must not re-offer the same action.
    #[test]
    fn standalone_extract_is_idempotent_on_repeat_request() {
        let actions = refactor_actions(STANDALONE_EXTRACT_FIXTURE, STANDALONE_BLOCK);
        let extracts = extract_function_actions(&actions);
        assert_eq!(extracts.len(), 1, "expected exactly one extract action");
        let edited = apply_action(STANDALONE_EXTRACT_FIXTURE, extracts[0]);
        let repeat = refactor_actions(&edited, "extracted_function();");
        assert!(
            extract_function_actions(&repeat).is_empty(),
            "repeat request must not offer extract-function again, got: {:?}",
            repeat.iter().map(|action| &action.title).collect::<Vec<_>>()
        );
    }
}
