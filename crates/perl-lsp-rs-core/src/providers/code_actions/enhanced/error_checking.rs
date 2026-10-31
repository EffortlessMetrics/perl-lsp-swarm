//! Error checking code actions

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind};

/// File-operation names that can receive an `or die` wrapper.
const FILE_OP_NAMES: &[&str] =
    &["open", "close", "print", "printf", "write", "read", "seek", "truncate"];

/// Add error checking to file operations
///
/// Offered only when the call is a full statement expression. The dispatcher
/// already visits `ExpressionStatement` nodes, so this gate uses that parent
/// node and does not need a dispatcher signature change.
pub fn add_error_checking(node: &Node, source: &str) -> Option<CodeAction> {
    let NodeKind::ExpressionStatement { expression } = &node.kind else {
        return None;
    };
    let NodeKind::FunctionCall { name, args: _ } = &expression.kind else {
        return None;
    };
    let func_name = name.as_str();
    if !FILE_OP_NAMES.contains(&func_name) {
        return None;
    }
    if has_error_checking_nearby(source, expression.location.end) {
        return None;
    }
    let expr_text = source.get(expression.location.start..expression.location.end)?;
    let call_text = wrap_paren_less_call(func_name, expr_text);

    Some(CodeAction {
        title: format!("Add error checking to '{}'", func_name),
        kind: CodeActionKind::RefactorRewrite,
        diagnostics: Vec::new(),
        edit: CodeActionEdit {
            changes: vec![TextEdit {
                location: expression.location,
                new_text: format!("{call_text} or die \"Failed to {func_name}: $!\""),
            }],
        },
        is_preferred: false,
    })
}

/// Wrap a paren-less call so `or die` binds to the call, not a trailing argument.
fn wrap_paren_less_call(func_name: &str, expr_text: &str) -> String {
    let Some(after_name) = expr_text.strip_prefix(func_name) else {
        return expr_text.to_string();
    };
    let rest = after_name.trim_start();
    if rest.starts_with('(') { expr_text.to_string() } else { format!("{func_name}({rest})") }
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
    use super::*;
    use perl_parser_core::Parser;
    use perl_parser_core::ast::{Node, NodeKind};
    use perl_tdd_support::{must, must_some};

    fn parse_ast(source: &str) -> Node {
        let mut parser = Parser::new(source);
        must(parser.parse())
    }

    fn walk_error_checking(node: &Node, source: &str, actions: &mut Vec<CodeAction>) {
        if let Some(action) = add_error_checking(node, source) {
            actions.push(action);
        }
        for child in node.children() {
            walk_error_checking(child, source, actions);
        }
    }

    fn collect_error_checking(source: &str) -> Vec<CodeAction> {
        let ast = parse_ast(source);
        let mut actions = Vec::new();
        walk_error_checking(&ast, source, &mut actions);
        actions
    }

    fn contains_function_call(node: &Node, name: &str) -> bool {
        match &node.kind {
            NodeKind::FunctionCall { name: call, .. } if call == name => true,
            _ => node.children().iter().any(|child| contains_function_call(child, name)),
        }
    }

    fn first_error_kind(node: &Node) -> Option<&'static str> {
        match &node.kind {
            NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock => Some(node.kind.kind_name()),
            _ => node.children().iter().copied().find_map(first_error_kind),
        }
    }

    fn apply_single_edit(source: &str, action: &CodeAction) -> String {
        let edit = must_some(action.edit.changes.first());
        let mut edited = source.to_string();
        edited.replace_range(edit.location.start..edit.location.end, &edit.new_text);
        edited
    }

    fn assert_clean_reparse(source: &str) {
        let ast = parse_ast(source);
        assert!(
            first_error_kind(&ast).is_none(),
            "edited source must reparse without Error/Missing nodes:\n{source}\nsexp:\n{}",
            ast.to_sexp()
        );
    }

    #[test]
    fn statement_level_parenless_open_wraps_and_reparses() {
        let source = "open my $fh, '<', $f;";
        let ast = parse_ast(source);
        assert!(
            contains_function_call(&ast, "open"),
            "test premise: parser must produce an open FunctionCall"
        );

        let actions = collect_error_checking(source);
        assert_eq!(actions.len(), 1, "statement-level open must offer one action");
        let edit = must_some(actions[0].edit.changes.first());
        assert_eq!(
            edit.new_text, "open(my $fh, '<', $f) or die \"Failed to open: $!\"",
            "paren-less open must wrap so or die binds to the call"
        );

        let edited = apply_single_edit(source, &actions[0]);
        assert_eq!(
            edited, "open(my $fh, '<', $f) or die \"Failed to open: $!\";",
            "replacement must keep the statement terminator"
        );
        assert_clean_reparse(&edited);
    }

    #[test]
    fn nested_if_condition_assignment_and_arg_list_offer_no_action() {
        let cases = [
            ("if", "if (open my $fh, '<', $f) { 1 }", "open"),
            ("assignment", "my $ok = close($fh);", "close"),
            ("arg list", "foo(close($fh));", "close"),
        ];
        for (label, source, name) in cases {
            let ast = parse_ast(source);
            assert!(
                contains_function_call(&ast, name),
                "{label}: test premise: parser must produce a {name} FunctionCall in `{source}`"
            );
            let actions = collect_error_checking(source);
            assert!(
                actions.is_empty(),
                "{label}: nested {name} must not offer error checking; got {actions:?}"
            );
        }
    }

    #[test]
    fn existing_or_die_nearby_offers_no_action() {
        let source = "open my $fh, '<', $f or die \"x\";";
        let ast = parse_ast(source);
        assert!(
            contains_function_call(&ast, "open"),
            "test premise: already-checked open must still be a FunctionCall"
        );
        let actions = collect_error_checking(source);
        assert!(
            actions.is_empty(),
            "existing or die nearby must suppress the action; got {actions:?}"
        );
    }
}
