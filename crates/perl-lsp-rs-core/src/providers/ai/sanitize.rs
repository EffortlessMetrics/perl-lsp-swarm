//! Output sanitization for AI completion candidates (#5049 item 7).
//!
//! Models routinely wrap completions in Markdown code fences despite
//! instructions. Ghost text must receive the raw completion only: this
//! module strips the fence wrapper so the downstream parse-safety seam
//! judges the actual candidate, not its packaging.
//!
//! Scope boundary: only a *leading* wrapper is recognized — the first
//! non-empty line of the output must open the fence. A line-initial fence
//! anywhere else (a here-doc body, a POD block, generated Markdown inside
//! the completion) is content: stripping it silently deleted valid source
//! while the surviving fragment still parsed, so the parse-safety seam
//! passed the corruption through. Prose-leading output is also returned
//! unchanged here — it cannot be separated from code reliably at this
//! layer, and the shared parse-safety filter rejects it before display.

/// Strip a leading Markdown code-fence wrapper from one completion candidate.
///
/// - Text whose first non-empty line does not open a ` ``` ` fence is
///   returned unchanged — including prose-leading output and completions
///   whose content merely contains fences.
/// - Text with a leading fenced block returns the block's inner content,
///   preserving internal indentation and stripping the final newline before
///   the closing fence.
/// - An unterminated leading fence (opening marker without a closing one,
///   common at streaming cutoffs) returns the content after the opening
///   fence line.
pub fn sanitize_completion_text(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    // Wrapper recognition anchors on the first non-empty line only: to be
    // packaging, a fence must open the output. Later line-initial fences
    // are content (here-doc bodies, POD), never a wrapper.
    let Some(open_idx) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return raw.to_string();
    };
    if !lines[open_idx].trim_start().starts_with("```") {
        // The output does not open with a fence: return it unchanged.
        // Interior fences are content, and prose-leading output falls
        // through to the parse-safety seam instead of being stripped here.
        return raw.to_string();
    }
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

/// True while a line could still grow into a fence marker: after leading
/// whitespace it is a non-empty run of backticks only.
fn incomplete_fence_marker_candidate(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'`')
}

/// Streaming-visible text for one cumulative chunk.
///
/// [`sanitize_completion_text`] is a boundary operation: it assumes the
/// output is complete. Applying it to every cumulative SSE chunk produces
/// observable artifacts — a partially delivered opening marker (`"``"`)
/// passes through raw as candidate text, and a partially delivered closing
/// marker surfaces as a stray backtick suffix for one tick.
///
/// Streaming holds back only the genuinely ambiguous regions:
///
/// - Before the first newline arrives, a bare backtick run may still become
///   the opening fence, so it is withheld until the wrapper decision can be
///   made.
/// - Once a leading wrapper is recognized, a trailing bare backtick run may
///   still grow into the closing marker, so it is withheld until it
///   resolves.
///
/// Content-anchored output is never stripped at the boundary, so its
/// interior fences stream raw. The completion boundary still emits the full
/// [`sanitize_completion_text`] result, which restores any held tail.
pub fn sanitize_streaming_text(cumulative: &str) -> String {
    let sanitized = sanitize_completion_text(cumulative);
    if !first_non_empty_line_opens_fence(cumulative) {
        // Content-anchored (or still-classifying) output: only the
        // in-flight first line can change the wrapper decision.
        if !cumulative.contains('\n') && incomplete_fence_marker_candidate(&sanitized) {
            return String::new();
        }
        return sanitized;
    }
    // Wrapper mode: hold a trailing bare backtick run — it may still grow
    // into the closing marker.
    let Some(last_newline) = sanitized.rfind('\n') else {
        return sanitized;
    };
    if incomplete_fence_marker_candidate(&sanitized[last_newline + 1..]) {
        sanitized[..last_newline].to_string()
    } else {
        sanitized
    }
}

/// True when the first non-empty line of `raw` opens a fence, i.e. the
/// boundary rule would treat the output as a leading wrapper.
fn first_non_empty_line_opens_fence(raw: &str) -> bool {
    raw.lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim_start().starts_with("```"))
}

#[cfg(test)]
mod tests {
    use super::{
        incomplete_fence_marker_candidate, sanitize_completion_text, sanitize_streaming_text,
    };

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
    fn prose_leading_output_is_not_reinterpreted_as_a_wrapper() {
        // Only an output that OPENS with a fence is packaging. Prose-leading
        // output falls through unchanged; the shared parse-safety seam owns
        // rejecting it, so stripping here can never destroy content.
        let raw =
            "Here is the completion:\n```perl\nmy $x = 1;\n```\nLet me know if you need more.";
        assert_eq!(sanitize_completion_text(raw), raw);
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

    #[test]
    fn heredoc_with_fenced_markdown_body_is_preserved() {
        // A line-initial fence inside a here-doc body is Perl content, not
        // response packaging. Stripping it deleted the assignment and both
        // delimiters while the surviving fragment still parsed as Perl.
        let raw = "my $doc = <<'EOF';\n# Usage\n```perl\nmy $x = 1;\n```\nEOF";
        assert_eq!(sanitize_completion_text(raw), raw);
    }

    #[test]
    fn pod_block_with_interior_fence_is_preserved() {
        // POD blocks frequently contain lines that begin with backticks;
        // they are content, not Markdown wrappers.
        let raw = "=pod\n\n```\n    some indented code\n```\n\n=cut";
        assert_eq!(sanitize_completion_text(raw), raw);
    }

    #[test]
    fn fence_after_leading_blank_lines_is_still_a_wrapper() {
        let raw = "\n\n```perl\nmy $x = 1;\n```";
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;");
    }

    #[test]
    fn streaming_holds_partial_opening_fence_marker() {
        // A cumulative bare backtick run may still be the opening marker
        // arriving; it must never surface as candidate text.
        assert_eq!(sanitize_streaming_text("`"), "");
        assert_eq!(sanitize_streaming_text("``"), "");
        assert_eq!(sanitize_streaming_text("```"), "");
        assert_eq!(sanitize_streaming_text("```p"), "");
    }

    #[test]
    fn streaming_holds_partial_closing_fence_marker() {
        assert_eq!(sanitize_streaming_text("```perl\nmy $x = 1;\n`"), "my $x = 1;");
        assert_eq!(sanitize_streaming_text("```perl\nmy $x = 1;\n``"), "my $x = 1;");
        assert_eq!(sanitize_streaming_text("```perl\nmy $x = 1;\n```"), "my $x = 1;");
    }

    #[test]
    fn streaming_content_fences_stream_raw_and_survive_the_boundary() {
        // Content-anchored output is never a wrapper: interior fences are
        // visible immediately (no hold) and the boundary preserves them.
        let raw = "print <<'EOF';\n```";
        assert_eq!(sanitize_streaming_text(raw), raw);
        assert_eq!(sanitize_completion_text(raw), raw);
    }

    #[test]
    fn streaming_midline_backticks_are_visible_immediately() {
        let raw = "my $marker = '```';\nreturn $marker;";
        assert_eq!(sanitize_streaming_text(raw), raw);
    }

    #[test]
    fn incomplete_marker_candidate_requires_bare_backtick_run() {
        assert!(incomplete_fence_marker_candidate("`"));
        assert!(incomplete_fence_marker_candidate("  ``"));
        assert!(!incomplete_fence_marker_candidate("```perl"));
        assert!(!incomplete_fence_marker_candidate("my $x;"));
        assert!(!incomplete_fence_marker_candidate(""));
    }
}
