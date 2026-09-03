//! Production line-ending inference for generated formatter output (#13792).
//!
//! Both the native formatter and the LSP whitespace projection must decide
//! which line ending to emit for text they generate — a final newline, or the
//! terminator for a line the source left unterminated. That decision is one
//! rule, so it lives here once rather than being copied per call site.
//!
//! `perl-lsp-rs-core`'s `providers::inline_completion::next_edit` now consumes
//! the position-aware helper as the shared authority for edits. Its
//! mixed-ending behavior is covered by next-edit regression tests, so future
//! synthesized insertions use the convention at their insertion point.
//!
//! `perl-position-tracking::detect_line_ending` is *not* a copy: it reports the
//! predominant style for position mapping, which is a different question.
//!
//! Authority boundary: this is deliberately **not**
//! [`crate::native::source_convention`]. That function belongs to the #8048
//! shift-left seam, has no production caller until #10239, and answers a
//! different question — it reports the last convention anywhere in the source
//! including a bare CR. [`inferred_line_ending`] answers the narrower
//! production question this crate has always answered: given that we are about
//! to synthesize a terminator, does this document use CRLF or LF? Bare CR is
//! not a generated terminator here; converging the two rules is #10239's work,
//! not this module's.

/// The line ending to use for text generated into `source`.
///
/// The document's convention is read from its last LF: an LF preceded by a CR
/// means the document is CRLF, anything else means LF. A source with no LF at
/// all has established no convention and falls back to LF, which also covers
/// the empty and short-buffer cases.
///
/// Bare CR is deliberately not inferred as a generated terminator — see the
/// module documentation for the authority boundary against
/// [`crate::native::source_convention`].
///
/// ```
/// use perl_lsp_perltidy::native::inferred_line_ending;
///
/// assert_eq!(inferred_line_ending("my $x = 1;\r\n"), "\r\n");
/// assert_eq!(inferred_line_ending("my $x = 1;\n"), "\n");
/// assert_eq!(inferred_line_ending("my $x = 1;"), "\n");
/// assert_eq!(inferred_line_ending(""), "\n");
/// ```
#[must_use]
pub fn inferred_line_ending(source: &str) -> &'static str {
    let bytes = source.as_bytes();
    let Some(last_lf) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return "\n";
    };

    if last_lf > 0 && bytes[last_lf - 1] == b'\r' { "\r\n" } else { "\n" }
}

/// The line ending to use for text generated at byte `offset` in `source`.
///
/// The preceding line ending is authoritative when the insertion point is
/// after a line terminator. At the beginning of a document, the first
/// following line ending is used instead. If neither exists, this falls back
/// to [`inferred_line_ending`]. Invalid UTF-8 boundaries are also handled by
/// that document-level fallback.
#[must_use]
pub fn inferred_line_ending_at(source: &str, offset: usize) -> &'static str {
    let bytes = source.as_bytes();
    if offset > 0 && offset < bytes.len() && bytes[offset - 1] == b'\r' && bytes[offset] == b'\n' {
        return "\r\n";
    }

    let Some(prefix) = source.get(..offset) else {
        return inferred_line_ending(source);
    };
    if let Some(last_lf) = prefix.as_bytes().iter().rposition(|byte| *byte == b'\n') {
        return if last_lf > 0 && prefix.as_bytes()[last_lf - 1] == b'\r' { "\r\n" } else { "\n" };
    }

    let Some(suffix) = source.get(offset..) else {
        return inferred_line_ending(source);
    };
    if let Some(first_lf) = suffix.as_bytes().iter().position(|byte| *byte == b'\n') {
        return if (first_lf > 0 && suffix.as_bytes()[first_lf - 1] == b'\r')
            || (first_lf == 0 && offset > 0 && source.as_bytes()[offset - 1] == b'\r')
        {
            "\r\n"
        } else {
            "\n"
        };
    }

    inferred_line_ending(source)
}

#[cfg(test)]
mod tests {
    use super::{inferred_line_ending, inferred_line_ending_at};

    #[test]
    fn empty_and_unterminated_sources_fall_back_to_lf() {
        assert_eq!(inferred_line_ending(""), "\n");
        assert_eq!(inferred_line_ending("my $x = 1;"), "\n");
        assert_eq!(inferred_line_ending("\r"), "\n");
        assert_eq!(inferred_line_ending("a\rb\rc"), "\n");
    }

    #[test]
    fn a_lone_leading_lf_is_lf_not_a_crlf_underflow() {
        // `last_lf == 0` must not index byte -1.
        assert_eq!(inferred_line_ending("\n"), "\n");
        assert_eq!(inferred_line_ending("\nmy $x = 1;"), "\n");
    }

    #[test]
    fn the_last_lf_decides_the_convention() {
        assert_eq!(inferred_line_ending("a\r\nb\n"), "\n");
        assert_eq!(inferred_line_ending("a\nb\r\n"), "\r\n");
        assert_eq!(inferred_line_ending("a\r\nb\nc"), "\n");
        assert_eq!(inferred_line_ending("a\nb\r\nc"), "\r\n");
    }

    #[test]
    fn crlf_is_recognized_anywhere_the_last_lf_lands() {
        assert_eq!(inferred_line_ending("\r\n"), "\r\n");
        assert_eq!(inferred_line_ending("my $x = 1;\r\n"), "\r\n");
        assert_eq!(inferred_line_ending("my $x = 1;\r\nmy $y = 2;"), "\r\n");
    }

    #[test]
    fn a_cr_not_adjacent_to_the_last_lf_does_not_make_the_document_crlf() {
        assert_eq!(inferred_line_ending("a\rb\n"), "\n");
        assert_eq!(inferred_line_ending("a\r\r\nb\n"), "\n");
    }

    #[test]
    fn multibyte_content_before_the_terminator_is_handled_by_byte_scan() {
        assert_eq!(inferred_line_ending("my $x = \"\u{e9}\";\r\n"), "\r\n");
        assert_eq!(inferred_line_ending("my $x = \"\u{1f600}\";\n"), "\n");
    }

    #[test]
    fn insertion_position_wins_over_a_different_document_tail_convention() {
        let source = "package Demo;\r\nmy $value = 1;\nmy $tail = 2;\r\n";
        let insertion = source.find("my $value").expect("fixture has insertion line");
        assert_eq!(inferred_line_ending_at(source, insertion), "\r\n");

        let later_insertion = source.find("my $tail").expect("fixture has later line");
        assert_eq!(inferred_line_ending_at(source, later_insertion), "\n");
    }

    #[test]
    fn beginning_insertion_uses_first_line_convention() {
        assert_eq!(inferred_line_ending_at("package Demo;\r\n", 0), "\r\n");
        assert_eq!(inferred_line_ending_at("package Demo;\n", 0), "\n");
    }

    #[test]
    fn an_offset_between_crlf_bytes_keeps_the_pair_together() {
        assert_eq!(inferred_line_ending_at("a\r\n", 2), "\r\n");
        assert_eq!(inferred_line_ending_at("a\nb\r\n", 4), "\r\n");
    }
}
