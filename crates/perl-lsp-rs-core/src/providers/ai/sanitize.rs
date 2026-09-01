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
///   preserving internal indentation and stripping only the single line
///   separator immediately before the closing fence.
/// - An unterminated leading fence (opening marker without a closing one,
///   common at streaming cutoffs) returns the content after the opening
///   fence line, preserving every body byte including a trailing newline.
/// - A closing marker must carry at least as many backticks as the opening
///   fence and nothing but fence whitespace, so a shorter line-initial
///   fence inside the body (a here-doc, say) cannot close the wrapper
///   early.
pub fn sanitize_completion_text(raw: &str) -> String {
    // Wrapper recognition anchors on the first non-empty line only: to be
    // packaging, a fence must open the output. Later line-initial fences
    // are content (here-doc bodies, POD), never a wrapper.
    let Some((open_start, open_end)) = first_non_empty_line_bounds(raw) else {
        return raw.to_string();
    };
    let Some(open_run) = opening_fence_run_length(&raw[open_start..open_end]) else {
        // The output does not open with a fence: return it unchanged.
        // Interior fences are content, and prose-leading output falls
        // through to the parse-safety seam instead of being stripped here.
        return raw.to_string();
    };
    // The body begins after the opening fence's line terminator; scan the
    // remaining lines for a real closing marker without reconstructing the
    // body through `str::lines`, which would drop the final newline.
    let body_start = line_terminator_end(raw, open_end);
    let mut cursor = body_start;
    while let Some(((start, end), next)) = line_bounds(raw, cursor) {
        if closing_fence_run_length(&raw[start..end], open_run).is_some() {
            // Preserve every body byte except the single separator
            // immediately before the real closing fence.
            let body = &raw[body_start..start];
            return trim_one_line_terminator(body).to_string();
        }
        cursor = next;
    }
    // Unterminated fence: keep everything after the opening line rather
    // than dropping the candidate entirely, including a trailing newline.
    raw[body_start..].to_string()
}

/// Byte bounds `(start, end)` of the first line whose trimmed content is
/// non-empty, terminator excluded. `None` while every line so far is blank.
fn first_non_empty_line_bounds(raw: &str) -> Option<(usize, usize)> {
    let mut cursor = 0;
    while let Some(((start, end), next)) = line_bounds(raw, cursor) {
        if !raw[start..end].trim().is_empty() {
            return Some((start, end));
        }
        cursor = next;
    }
    None
}

/// Byte bounds `(start, end)` of the line beginning at `cursor` plus the
/// cursor for the following line. The line content excludes its `\r\n` or
/// `\n` terminator; `next` always advances past it, so scans terminate.
fn line_bounds(raw: &str, cursor: usize) -> Option<((usize, usize), usize)> {
    if cursor >= raw.len() {
        return None;
    }
    match raw[cursor..].find('\n') {
        Some(pos) => {
            let line_end = cursor + pos;
            let next = line_end + 1;
            let content_end = if line_end > cursor && raw.as_bytes()[line_end - 1] == b'\r' {
                line_end - 1
            } else {
                line_end
            };
            Some(((cursor, content_end), next))
        }
        None => Some(((cursor, raw.len()), raw.len())),
    }
}

/// Offset just past the line terminator at `line_end`.
fn line_terminator_end(raw: &str, line_end: usize) -> usize {
    if raw[line_end..].starts_with("\r\n") {
        line_end + 2
    } else if raw[line_end..].starts_with('\n') || raw[line_end..].starts_with('\r') {
        line_end + 1
    } else {
        line_end
    }
}

/// Remove one trailing line terminator (`\r\n`, `\n`, or `\r`).
fn trim_one_line_terminator(body: &str) -> &str {
    let mut end = body.len();
    if body.ends_with("\r\n") {
        end -= 2;
    } else if body.ends_with('\n') || body.ends_with('\r') {
        end -= 1;
    }
    &body[..end]
}

/// Backtick run length of one line's opening fence marker, if any.
///
/// The line must start (after leading whitespace) with at least three
/// backticks; an opening marker may carry an info string after the run.
fn opening_fence_run_length(line: &str) -> Option<usize> {
    let run = leading_backtick_run(line);
    if run >= 3 { Some(run) } else { None }
}

/// Backtick run length of one line's closing fence marker, if any.
///
/// A closing marker must carry at least `open_run` backticks and nothing
/// but fence whitespace after the run (CommonMark rule), so a shorter
/// content fence cannot close a longer wrapper and a marker carrying an
/// info string is content, not a closer.
fn closing_fence_run_length(line: &str, open_run: usize) -> Option<usize> {
    let trimmed = line.trim_start();
    let run = trimmed.bytes().take_while(|byte| *byte == b'`').count();
    if run >= open_run && trimmed[run..].trim().is_empty() { Some(run) } else { None }
}

/// Count the backticks starting the line, after leading whitespace.
fn leading_backtick_run(line: &str) -> usize {
    line.trim_start().bytes().take_while(|byte| *byte == b'`').count()
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
/// Streaming holds back only the genuinely ambiguous regions, so every
/// non-final emitted value stays a prefix of every later value:
///
/// - While no non-empty line has arrived, leading whitespace may still
///   precede the opening fence, so the wrapper decision is pending and
///   nothing is emitted yet.
/// - Before the first newline arrives, a bare backtick run may still become
///   the opening fence, so it is withheld until the wrapper decision can be
///   made.
/// - Once a leading wrapper is recognized, the trailing line separators may
///   include the one the boundary strips together with the closing marker,
///   and a trailing bare backtick run may still grow into that marker, so
///   both are withheld until they resolve.
///
/// Content-anchored output is never stripped at the boundary, so its
/// interior fences stream raw. The completion boundary still emits the full
/// [`sanitize_completion_text`] result, which restores any held tail.
pub fn sanitize_streaming_text(cumulative: &str) -> String {
    let sanitized = sanitize_completion_text(cumulative);
    let Some((open_start, open_end)) = first_non_empty_line_bounds(cumulative) else {
        // Only blank lines so far: the wrapper decision is still pending
        // and the blank prefix may yet precede an opening fence.
        return String::new();
    };
    if !first_non_empty_line_opens_fence(cumulative) {
        // Content-anchored (or still-classifying) output: while the first
        // non-empty line is still in flight, it can still grow into the
        // opening fence, so a bare backtick run stays withheld.
        let line_in_flight = !cumulative[open_end..].contains('\n');
        if line_in_flight && incomplete_fence_marker_candidate(&cumulative[open_start..]) {
            return String::new();
        }
        return sanitized;
    }
    // Wrapper mode: while the wrapper is still open, hold the ambiguous
    // tail — trailing line separators may include the one the boundary
    // strips with the closing marker, and a trailing bare backtick run may
    // still grow into that marker. Once a valid closing marker has arrived,
    // the boundary value is final-shaped and streams as-is.
    if !wrapper_closes(cumulative) {
        let bytes = sanitized.as_bytes();
        let mut end = sanitized.len();
        // Trailing CR/LF bytes: one of them could become (part of) the
        // single separator the boundary strips with the closing marker, so
        // a half-delivered "\r" is held exactly like a "\n".
        while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
            end -= 1;
        }
        // Trailing bare backtick run: it may still grow into the closing
        // marker, and the separator in front of it would be stripped with it.
        let line_start = sanitized[..end].rfind('\n').map_or(0, |pos| pos + 1);
        if incomplete_fence_marker_candidate(&sanitized[line_start..end]) {
            end = line_start;
            if end > 0 && bytes[end - 1] == b'\n' {
                end -= 1;
                if end > 0 && bytes[end - 1] == b'\r' {
                    end -= 1;
                }
            }
        }
        return sanitized[..end].to_string();
    }
    sanitized
}

/// True when the output opens a leading wrapper whose real closing marker
/// has already arrived in `raw`.
fn wrapper_closes(raw: &str) -> bool {
    let Some((open_start, open_end)) = first_non_empty_line_bounds(raw) else {
        return false;
    };
    let Some(open_run) = opening_fence_run_length(&raw[open_start..open_end]) else {
        return false;
    };
    let mut cursor = line_terminator_end(raw, open_end);
    while let Some(((start, end), next)) = line_bounds(raw, cursor) {
        if closing_fence_run_length(&raw[start..end], open_run).is_some() {
            return true;
        }
        cursor = next;
    }
    false
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

    /// Stream `raw` one character at a time and assert the cumulative-prefix
    /// contract: no emission is ever retracted by a later chunk.
    fn assert_stream_monotone(raw: &str) -> Vec<String> {
        let mut emissions: Vec<String> = Vec::new();
        let mut cumulative = String::new();
        for ch in raw.chars() {
            cumulative.push(ch);
            emissions.push(sanitize_streaming_text(&cumulative));
        }
        for window in emissions.windows(2) {
            assert!(
                window[1].starts_with(&window[0]),
                "streamed candidate was retracted: {:?} -> {:?}",
                window[0],
                window[1]
            );
        }
        emissions
    }

    #[test]
    fn streaming_blank_prefix_before_fence_never_retracts() {
        // Leading blank lines before the opening fence: the pending wrapper
        // decision must never emit text the boundary would take back.
        let raw = "\n\n```perl\nmy $x = 1;\n```";
        let emissions = assert_stream_monotone(raw);
        assert_eq!(emissions.last(), Some(&"my $x = 1;".to_string()));
    }

    #[test]
    fn streaming_indented_opening_fence_never_retracts() {
        // Whitespace, then an indented opening fence: the whole blank
        // prefix stays withheld until the first non-empty line resolves.
        let raw = "  \n```perl\nmy $x = 1;\n```";
        let emissions = assert_stream_monotone(raw);
        assert_eq!(emissions.last(), Some(&"my $x = 1;".to_string()));
    }

    #[test]
    fn streaming_blank_line_before_closer_never_retracts() {
        // A blank body line right before the closing marker: the streamed
        // prefix never includes text the boundary strips, and the final
        // streamed value (wrapper closed) equals the boundary output.
        let raw = "```perl\nmy $x = 1;\n\n```";
        let emissions = assert_stream_monotone(raw);
        assert_eq!(emissions.last(), Some(&"my $x = 1;\n".to_string()));
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;\n");
    }

    #[test]
    fn unterminated_fence_boundary_preserves_trailing_newline() {
        // A truncated fenced response that ends on a newline keeps it, so
        // the completion does not join the document's existing line.
        assert_eq!(sanitize_completion_text("```perl\nmy $x = 1;\n"), "my $x = 1;\n");
        assert_eq!(sanitize_completion_text("```perl\r\nmy $x = 1;\r\n"), "my $x = 1;\r\n");
    }

    #[test]
    fn closed_fence_strips_only_the_separator_before_the_marker() {
        // Interior blank lines stay in the body; exactly one separator
        // immediately before the real closing marker is removed.
        assert_eq!(sanitize_completion_text("```perl\nmy $x = 1;\n\n```"), "my $x = 1;\n");
        assert_eq!(sanitize_completion_text("```perl\na;\n\n\nb;\n```"), "a;\n\n\nb;");
    }

    #[test]
    fn longer_wrapper_tolerates_short_content_fences() {
        // A four-backtick wrapper around Perl whose body contains a heredoc
        // with three-backtick Markdown fences: the short fence is content
        // and must not close the wrapper early.
        let raw = "````perl\nmy $doc = <<'END';\n```\nEND\nreturn $doc;\n````";
        assert_eq!(sanitize_completion_text(raw), "my $doc = <<'END';\n```\nEND\nreturn $doc;");
    }

    #[test]
    fn streaming_longer_wrapper_holds_until_the_real_closer() {
        let raw = "````perl\nmy $doc = <<'END';\n```\nEND\nreturn $doc;\n````";
        let emissions = assert_stream_monotone(raw);
        assert_eq!(
            emissions.last(),
            Some(&"my $doc = <<'END';\n```\nEND\nreturn $doc;".to_string())
        );
    }

    #[test]
    fn closing_fence_with_info_string_is_not_a_closer() {
        // A marker carrying fence info text is content, never a closing
        // marker, so the wrapper stays open (unterminated at this cut).
        let raw = "```perl\nmy $x = 1;\n```perl\n";
        assert_eq!(sanitize_completion_text(raw), "my $x = 1;\n```perl\n");
    }

    #[test]
    fn generated_stream_matrix_stays_monotone_and_terminates() {
        // Termination and the cumulative-prefix contract must hold across
        // the line-ending x blank-prefix x wrapper x content x tail matrix,
        // not only the hand-picked cases above. A regression here either
        // hangs (scan that cannot advance) or trips the retraction assert.
        let line_endings = ["\n", "\r\n"];
        let blank_prefixes = ["", "\n", "  \n", "\r\n"];
        let wrappers = ["```perl\n", "````\n"];
        let bodies = ["my $x = 1;", "a;\n\nb;\n", "print <<'E';\n```\nE;"];
        let tails = ["", "```", "```\n", "\n```", "````"];
        for le in line_endings {
            for blank in blank_prefixes {
                for wrapper in wrappers {
                    for body in bodies {
                        for tail in tails {
                            let raw = format!("{blank}{wrapper}{body}{le}{tail}");
                            // Panics on retraction; hangs on a stuck scan.
                            let _ = assert_stream_monotone(&raw);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn generated_boundary_matches_crlf_and_lf_streams() {
        // Both line endings must produce the same sanitized body: the
        // wrapper strip may differ only in the removed separator bytes.
        let lf = sanitize_completion_text("```perl\na;\nb;\n```");
        let crlf = sanitize_completion_text("```perl\r\na;\r\nb;\r\n```");
        assert_eq!(lf, "a;\nb;");
        assert_eq!(crlf, "a;\r\nb;");
        // Unterminated: the trailing separator is body and survives.
        assert_eq!(sanitize_completion_text("```perl\na;\n"), "a;\n");
        assert_eq!(sanitize_completion_text("```perl\r\na;\r\n"), "a;\r\n");
    }
}
