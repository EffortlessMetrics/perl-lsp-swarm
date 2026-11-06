//! Error checking code actions

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind};

/// File operations that can receive an `or die` suffix.
const FILE_OPS: &[&str] =
    &["open", "close", "print", "printf", "write", "read", "seek", "truncate"];

/// Add error checking to file operations
///
/// Offered only when the file-op `FunctionCall` is the entire statement
/// expression (`ExpressionStatement`). Nested calls (`if (...)`, assignment
/// RHS, argument lists) are skipped: appending `or die` there changes
/// precedence or produces invalid Perl (#18442 item 3).
///
/// The dispatcher already visits `ExpressionStatement` nodes, so the
/// statement-level gate lives here without a parent-pointer change.
pub fn add_error_checking(node: &Node, source: &str) -> Option<CodeAction> {
    let (call, func_name) = statement_level_file_op(node)?;
    if has_error_checking_nearby(source, call.location.end) {
        return None;
    }
    let expr_text = source.get(call.location.start..call.location.end)?;
    Some(CodeAction {
        title: format!("Add error checking to '{}'", func_name),
        kind: CodeActionKind::RefactorRewrite,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![TextEdit {
                location: call.location,
                new_text: wrap_call_with_or_die(func_name, expr_text),
            }],
        },
        is_preferred: false,
    })
}

/// A file-op call that is the entire `ExpressionStatement` expression.
fn statement_level_file_op(node: &Node) -> Option<(&Node, &str)> {
    let NodeKind::ExpressionStatement { expression } = &node.kind else {
        return None;
    };
    match &expression.kind {
        NodeKind::FunctionCall { name, .. } if FILE_OPS.contains(&name.as_str()) => {
            Some((expression, name.as_str()))
        }
        _ => None,
    }
}

/// Bind `or die` to the call. Paren-less forms such as `open my $fh, '<', $f`
/// become `open(...)` so the suffix cannot be absorbed as a list argument.
fn wrap_call_with_or_die(func_name: &str, expr_text: &str) -> String {
    let Some(after_name) = expr_text.get(func_name.len()..) else {
        return format!("{expr_text} or die \"Failed to {func_name}: $!\"");
    };
    let trimmed = after_name.trim_start();
    if trimmed.starts_with('(') {
        format!("{expr_text} or die \"Failed to {func_name}: $!\"")
    } else {
        format!("{func_name}({trimmed}) or die \"Failed to {func_name}: $!\"")
    }
}

/// Number of characters of lookahead scanned for an existing error-checking idiom.
const LOOKAHEAD_CHARS: usize = 50;

/// Check if there's error checking nearby
pub fn has_error_checking_nearby(source: &str, pos: usize) -> bool {
    // Scan the next `LOOKAHEAD_CHARS` *characters* for "or", "||", "die", "warn".
    //
    // The window end is measured in characters (not bytes) so it can never bisect a
    // multi-byte character — the fixed 50-*byte* window this replaced panicked
    // whenever non-ASCII text followed a file operation within 50 bytes (#9835).
    //
    // `pos` comes from a node end offset, which is a valid boundary within the
    // source the AST was parsed from. Taking the window start with `get` rather
    // than indexing keeps this function total for any `(source, pos)` pair anyway,
    // so it is the window width — not the caller — that decides the result. Note
    // this makes only *this* function total: the surrounding walk still indexes
    // `source` by raw node offsets in several places (#13874).
    let Some(rest) = source.get(pos..) else {
        return false;
    };
    let end = rest.char_indices().nth(LOOKAHEAD_CHARS).map_or(rest.len(), |(idx, _)| idx);
    let check_text = &rest[..end];
    check_text.contains(" or ")
        || check_text.contains(" || ")
        || check_text.contains("die")
        || check_text.contains("warn")
}

#[cfg(test)]
mod utf8_boundary_tests {
    use super::*;
    use perl_tdd_support::must_some;

    /// The lookahead window must not bisect a multi-byte character.
    ///
    /// `pos` is a valid boundary, but `pos + 50` bytes lands inside an 'é',
    /// which panicked the `textDocument/codeAction` request before #9835.
    ///
    /// Every valid boundary is exercised rather than one hand-picked offset:
    /// for a 2-byte character, half of the candidate offsets happen to leave
    /// `pos + 50` on a boundary, so a single offset can pass by luck.
    #[test]
    fn lookahead_window_does_not_panic_inside_multibyte_char() {
        for filler in ["é", "→", "😀"] {
            let source = format!("open my $fh, '<', 'f';{}", filler.repeat(40));
            for pos in (0..=source.len()).filter(|&i| source.is_char_boundary(i)) {
                assert!(
                    !has_error_checking_nearby(&source, pos),
                    "filler {filler:?} at pos {pos} contains no error-checking idiom"
                );
            }
        }
    }

    /// A `pos` that is itself mid-character is refused rather than panicking.
    #[test]
    fn mid_char_pos_does_not_panic() {
        let source = "open my $fh, '<', 'é';";
        // 'é' occupies bytes 19..21, so 20 bisects it.
        assert!(!source.is_char_boundary(20), "test premise: byte 20 bisects 'é'");
        assert!(!has_error_checking_nearby(source, 20));
    }

    /// A `pos` past the end of the source is refused rather than panicking.
    #[test]
    fn out_of_range_pos_does_not_panic() {
        let source = "open my $fh, '<', 'f';";
        assert!(!has_error_checking_nearby(source, source.len() + 1_000));
    }

    /// `pos` exactly at the end yields an empty window, not a panic.
    #[test]
    fn pos_at_end_of_source_is_empty_window() {
        let source = "open my $fh, '<', 'f' or die;";
        assert!(!has_error_checking_nearby(source, source.len()));
    }

    /// Negative control: the boundary-safe window still detects error checking.
    #[test]
    fn lookahead_window_still_finds_error_checking() {
        let source = "open my $fh, '<', 'f' or die \"no: $!\";";
        // Derive the scan start from the fixture rather than hard-coding a byte
        // index, so the assertion stays honest if the fixture is ever edited.
        let pos = must_some(source.find(" or "));
        assert!(has_error_checking_nearby(source, pos));
    }

    /// The window is bounded: an idiom beyond 50 characters must not be seen.
    ///
    /// This is the discriminating control for the byte/char change — the window
    /// counts characters, so a `die` at character 51 stays out of range while a
    /// `die` at character 49 stays in range.
    #[test]
    fn lookahead_window_is_bounded_in_characters() {
        let inside = format!("{}die", "é".repeat(47));
        assert!(
            has_error_checking_nearby(&inside, 0),
            "'die' starting at character 47 is inside the 50-character window"
        );

        let outside = format!("{}die", "é".repeat(60));
        assert!(
            !has_error_checking_nearby(&outside, 0),
            "'die' starting at character 60 is outside the 50-character window"
        );
    }
}

#[cfg(test)]
mod statement_level_tests {
    use super::super::EnhancedCodeActionsProvider;
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::{must, must_some};

    fn actions_for(source: &str) -> Vec<CodeAction> {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        let provider = EnhancedCodeActionsProvider::new(source.to_string());
        provider.get_enhanced_refactoring_actions(&ast, (0, source.len()))
    }

    fn error_checking_action(source: &str) -> Option<CodeAction> {
        actions_for(source).into_iter().find(|a| a.title.contains("error checking"))
    }

    fn apply_first_edit(source: &str, action: &CodeAction) -> String {
        let edit = must_some(action.edit.changes.first());
        let mut out = String::new();
        if let Some(prefix) = source.get(..edit.location.start) {
            out.push_str(prefix);
        }
        out.push_str(&edit.new_text);
        if let Some(suffix) = source.get(edit.location.end..) {
            out.push_str(suffix);
        }
        out
    }

    fn assert_reparses(source: &str) {
        let mut parser = Parser::new(source);
        must(parser.parse());
    }

    /// #18442 item 3: statement-level paren-less `open` must become a
    /// call-bounded `or die` that the parser accepts.
    #[test]
    fn statement_level_parenless_open_wraps_and_reparses() {
        let source = "open my $fh, '<', $f;";
        let action = must_some(error_checking_action(source));
        assert!(
            action.title.contains("open"),
            "expected an open-targeted action, got {}",
            action.title
        );
        let edit = must_some(action.edit.changes.first());
        assert!(
            edit.new_text.starts_with("open(") && edit.new_text.contains(") or die"),
            "paren-less open must be wrapped so `or die` binds to the call, got {:?}",
            edit.new_text
        );
        let updated = apply_first_edit(source, &action);
        assert!(
            updated.contains("open(my $fh, '<', $f) or die"),
            "applied edit should keep the original args inside the call: {updated:?}"
        );
        assert_reparses(&updated);
    }

    /// Nested file-op calls must not receive `or die`; appending there
    /// changes precedence or produces invalid Perl.
    #[test]
    fn nested_file_ops_do_not_offer_error_checking() {
        for source in ["if (open($fh, '<', $f)) { }", "my $ok = close($fh);", "foo(close($fh));"] {
            assert!(
                error_checking_action(source).is_none(),
                "nested/non-statement call must not offer error checking: {source}"
            );
        }
    }

    /// Existing nearby `or die` remains a negative control.
    #[test]
    fn existing_or_die_nearby_suppresses_action() {
        let source = "open my $fh, '<', $f or die \"x\";";
        assert!(
            error_checking_action(source).is_none(),
            "already-checked open must not offer another or die"
        );
    }

    /// Parenthesized statement-level calls keep their parens and still reparse.
    #[test]
    fn statement_level_parenthesized_open_keeps_parens_and_reparses() {
        let source = "open($fh, '<', 'f');";
        let action = must_some(error_checking_action(source));
        let edit = must_some(action.edit.changes.first());
        assert!(
            edit.new_text.starts_with("open(") && edit.new_text.contains(") or die"),
            "parenthesized open should keep its parens, got {:?}",
            edit.new_text
        );
        assert!(
            !edit.new_text.starts_with("open(("),
            "must not wrap an already-parenthesized call: {:?}",
            edit.new_text
        );
        let updated = apply_first_edit(source, &action);
        assert_reparses(&updated);
    }
}
