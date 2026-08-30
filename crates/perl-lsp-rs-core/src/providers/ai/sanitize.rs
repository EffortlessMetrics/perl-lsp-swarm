//! Output sanitization for AI completion candidates (#5049 item 7).
//!
//! Models routinely wrap completions in Markdown code fences despite
//! instructions, or add prose around them. Ghost text must receive the raw
//! completion only: this module strips the fence wrapper so the downstream
//! parse-safety seam judges the actual candidate, not its packaging.
//!
//! Scope boundary: only a fence wrapper is recognized. Prose without any
//! fence is left untouched here — it cannot be separated from code
//! reliably at this layer, and the shared parse-safety filter rejects it
//! before display.

/// Strip a Markdown code-fence wrapper from one completion candidate.
///
/// - Text without any line that starts with ` ``` ` is returned unchanged.
/// - Text with a fenced block returns the first fenced block's inner
///   content, preserving internal indentation and stripping the final
///   newline before the closing fence.
/// - An unterminated fence (opening marker without a closing one) returns
///   the content after the opening fence line.
///
/// Everything outside the fence (prose, apologies, restated context) is
/// dropped.
pub fn sanitize_completion_text(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    // Only a line that begins with the fence marker (leading whitespace
    // allowed) opens a block. Backticks mid-line are ordinary Perl content,
    // not a wrapper.
    let Some(open_idx) = lines.iter().position(|line| line.trim_start().starts_with("```")) else {
        return raw.to_string();
    };
    let close_idx = lines
        .iter()
        .skip(open_idx + 1)
        .position(|line| line.trim_start().starts_with("```"))
        .map(|rel| open_idx + 1 + rel);
    let body = match close_idx {
        Some(close) => &lines[open_idx + 1..close],
        // Unterminated fence: keep everything after the opening line rather
        // than dropping the candidate entirely.
        None => &lines[open_idx + 1..],
    };
    body.join("\n")
}

#[cfg(test)]
mod tests {
    use super::sanitize_completion_text;

    #[test]
    fn plain_code_is_unchanged() {
        let raw = "my $x = 1;\nreturn $x;";
        assert_eq!(sanitize_completion_text(raw), raw);
    }

    #[test]
    fn fenced_block_with_language_tag_is_stripped() {
        assert_eq!(sanitize_completion_text("```perl\nmy $x = 1;\n```"), "my $x = 1;");
    }

    #[test]
    fn prose_around_fence_is_dropped() {
        let raw =
            "Here is the completion:\n```perl\nmy $x = 1;\n```\nLet me know if you need more.";
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;");
    }

    #[test]
    fn internal_indentation_is_preserved() {
        let raw = "```\nif ($ready) {\n    return 1;\n}\n```";
        assert_eq!(sanitize_completion_text(raw), "if ($ready) {\n    return 1;\n}");
    }

    #[test]
    fn unterminated_fence_keeps_remaining_content() {
        let raw = "```perl\nmy $x = 1;";
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;");
    }

    #[test]
    fn empty_fence_yields_empty_text() {
        assert_eq!(sanitize_completion_text("```perl\n```"), "");
    }

    #[test]
    fn midline_backticks_in_code_are_not_a_fence() {
        let raw = "my $marker = '```';\nreturn $marker;";
        assert_eq!(sanitize_completion_text(raw), raw);
    }

    #[test]
    fn crlf_fenced_block_is_normalized_and_stripped() {
        let raw = "```perl\r\nmy $x = 1;\r\n```";
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;");
    }
}
