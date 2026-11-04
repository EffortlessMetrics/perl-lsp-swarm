//! Refactoring actions for code transformations
//!
//! Provides automated refactoring operations for improving code structure.

use super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser::ast_utils::{find_node_at_range, find_statement_start};
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
    if actions.is_empty()
        && let Some(node) = find_node_at_range(ast, range)
        && matches!(&node.kind, NodeKind::FunctionCall { .. } | NodeKind::Binary { .. })
    {
        actions.push(CodeAction {
            title: "Extract to variable".to_string(),
            kind: CodeActionKind::RefactorExtract,
            diagnostics: Vec::new(),
            edit: extract_variable(source, node, range),
            is_preferred: false,
        });
    }

    actions
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
    /// block. Enhanced extract-subroutine declines this control-flow body, so
    /// current main's basic fallback is the only extract-function producer.
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
        assert_eq!(extract.kind, CodeActionKind::RefactorExtract);
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
}
