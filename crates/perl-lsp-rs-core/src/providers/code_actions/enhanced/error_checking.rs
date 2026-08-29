//! Error checking code actions

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind};

/// Add error checking to file operations
pub fn add_error_checking(node: &Node, source: &str) -> Option<CodeAction> {
    if let NodeKind::FunctionCall { name, args: _ } = &node.kind {
        let func_name = name.as_str();

        // Check for file operations without error checking
        if matches!(
            func_name,
            "open" | "close" | "print" | "printf" | "write" | "read" | "seek" | "truncate"
        ) {
            // Check if already has error checking
            if !has_error_checking_nearby(source, node.location.end) {
                let expr_text = &source[node.location.start..node.location.end];

                return Some(CodeAction {
                    title: format!("Add error checking to '{}'", func_name),
                    kind: CodeActionKind::RefactorRewrite,
                    diagnostics: Vec::new(),
                    edit: CodeActionEdit {
                        changes: vec![TextEdit {
                            location: node.location,
                            new_text: format!(
                                "{} or die \"Failed to {}: $!\"",
                                expr_text, func_name
                            ),
                        }],
                    },
                    is_preferred: false,
                });
            }
        }
    }

    None
}

/// Number of characters of lookahead scanned for an existing error-checking idiom.
const LOOKAHEAD_CHARS: usize = 50;

/// Check if there's error checking nearby
pub fn has_error_checking_nearby(source: &str, pos: usize) -> bool {
    // Scan the next `LOOKAHEAD_CHARS` *characters* for "or", "||", "die", "warn".
    //
    // `pos` is normally a node end offset, but a stale AST against edited text can
    // supply an offset that is past the end or inside a multi-byte sequence, so the
    // window start is taken with `get` rather than indexed. The window end is
    // measured in characters (not bytes) so it can never bisect a multi-byte
    // character — the fixed 50-*byte* window this replaced panicked whenever
    // non-ASCII text followed a file operation within 50 bytes.
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
        assert!(has_error_checking_nearby(source, 21));
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
