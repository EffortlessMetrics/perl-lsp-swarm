//! Line-ending inference for generated formatter output.
//!
//! Formatting frequently emits text the document did not previously contain: a
//! final newline, or a projected replacement for an admitted range. That
//! generated text must adopt the document's own terminator instead of a
//! hard-coded `\n`, or formatting silently converts a CRLF document to mixed
//! endings.
//!
//! This module is the single authority for that decision. Both the native
//! formatter in this crate and the LSP whitespace-options projection in
//! `perl-lsp-rs-core` route through [`generated_line_ending`], so the two
//! surfaces cannot drift apart (#13792).

/// Infer the line terminator that generated text should use for `source`.
///
/// The document's **last** terminator decides. A document already converted to
/// CRLF therefore keeps producing CRLF even while it still contains earlier
/// LF-only lines, which is what makes the inference stable under incremental
/// edits that rewrite a document's endings front-to-back.
///
/// The result is always `"\r\n"` or `"\n"` — never a bare `"\r"`. A source with
/// no `\n` at all (empty, single-line, or classic-Mac CR-only) falls back to
/// `"\n"`, because there is no evidence of a CRLF document and the formatter
/// does not generate classic-Mac endings.
///
/// # Examples
///
/// ```
/// use perl_lsp_perltidy::native::generated_line_ending;
///
/// assert_eq!(generated_line_ending("my $x = 1;\n"), "\n");
/// assert_eq!(generated_line_ending("my $x = 1;\r\n"), "\r\n");
///
/// // No terminator at all falls back to LF.
/// assert_eq!(generated_line_ending(""), "\n");
/// assert_eq!(generated_line_ending("my $x = 1;"), "\n");
///
/// // A bare CR is not a terminator this formatter generates.
/// assert_eq!(generated_line_ending("my $x = 1;\r"), "\n");
///
/// // The last terminator wins, not the first or the most common.
/// assert_eq!(generated_line_ending("a\r\nb\n"), "\n");
/// assert_eq!(generated_line_ending("a\nb\r\n"), "\r\n");
/// ```
#[must_use]
pub fn generated_line_ending(source: &str) -> &'static str {
    let bytes = source.as_bytes();
    let Some(last_lf) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return "\n";
    };

    if last_lf > 0 && bytes[last_lf - 1] == b'\r' { "\r\n" } else { "\n" }
}

#[cfg(test)]
mod tests {
    use super::generated_line_ending;
    use proptest::prelude::*;

    /// The crate sets `doctest = false`, so the module examples are not
    /// executable proof. Mirror each one here so the documented contract
    /// cannot drift away from the implementation unnoticed.
    #[test]
    fn documented_examples_hold() {
        assert_eq!(generated_line_ending("my $x = 1;\n"), "\n");
        assert_eq!(generated_line_ending("my $x = 1;\r\n"), "\r\n");
        assert_eq!(generated_line_ending(""), "\n");
        assert_eq!(generated_line_ending("my $x = 1;"), "\n");
        assert_eq!(generated_line_ending("my $x = 1;\r"), "\n");
        assert_eq!(generated_line_ending("a\r\nb\n"), "\n");
        assert_eq!(generated_line_ending("a\nb\r\n"), "\r\n");
    }

    #[test]
    fn short_buffers_fall_back_to_lf() {
        for source in ["", "\r", "x", "\r\r", "no newline here"] {
            assert_eq!(generated_line_ending(source), "\n", "source {source:?}");
        }
    }

    #[test]
    fn a_leading_lf_is_never_read_as_crlf() {
        // `last_lf == 0` has no preceding byte to inspect; the lookbehind must
        // not wrap around or index out of bounds.
        assert_eq!(generated_line_ending("\n"), "\n");
        assert_eq!(generated_line_ending("\na\r\rb"), "\n");
    }

    #[test]
    fn the_last_terminator_decides_not_the_majority() {
        // Three LF lines followed by one CRLF line still generates CRLF: the
        // inference is "what is this document being written as now", not a vote.
        assert_eq!(generated_line_ending("a\nb\nc\nd\r\n"), "\r\n");
        assert_eq!(generated_line_ending("a\r\nb\r\nc\r\nd\n"), "\n");
    }

    #[test]
    fn trailing_text_after_the_last_terminator_is_ignored() {
        assert_eq!(generated_line_ending("a\r\nb"), "\r\n");
        assert_eq!(generated_line_ending("a\nb"), "\n");
    }

    #[test]
    fn multibyte_text_around_the_terminator_is_handled() {
        // `\r` and `\n` are ASCII, so a UTF-8 continuation byte can never be
        // mistaken for either; assert that on real multi-byte content.
        assert_eq!(generated_line_ending("my $s = 'héllo → ☃';\r\n"), "\r\n");
        assert_eq!(generated_line_ending("my $s = 'héllo → ☃';\n"), "\n");
        assert_eq!(generated_line_ending("my $s = 'héllo → ☃';"), "\n");
    }

    proptest! {
        /// The result is one of exactly two terminators for any input.
        #[test]
        fn result_is_always_lf_or_crlf(source in "(?s).*") {
            let ending = generated_line_ending(&source);
            prop_assert!(ending == "\n" || ending == "\r\n", "got {ending:?}");
        }

        /// A source containing no `\n` has no CRLF evidence and must fall back.
        #[test]
        fn sources_without_lf_fall_back_to_lf(source in "[^\n]*") {
            prop_assert_eq!(generated_line_ending(&source), "\n");
        }

        /// An appended CRLF is authoritative whatever the prefix used.
        #[test]
        fn an_appended_crlf_wins(prefix in "(?s).*") {
            let source = format!("{prefix}\r\n");
            prop_assert_eq!(generated_line_ending(&source), "\r\n");
        }

        /// An appended LF is authoritative, provided it is a bare LF: a prefix
        /// ending in `\r` would make the appended byte the LF half of a CRLF,
        /// so it is excluded rather than asserted away.
        #[test]
        fn an_appended_bare_lf_wins(prefix in "(?s)(.*[^\r])?") {
            let source = format!("{prefix}\n");
            prop_assert_eq!(generated_line_ending(&source), "\n");
        }

        /// A `(?s).*` strategy only lands on `\r\n` in well under 1% of cases,
        /// so the properties above barely exercise CRLF. Build the document
        /// from explicit (body, terminator) pairs instead, and take the
        /// expectation from how it was *constructed* rather than from a second
        /// copy of the rule under test — an independent oracle, not a mirror.
        #[test]
        fn the_terminator_of_the_last_built_line_decides(
            lines in proptest::collection::vec(
                ("[^\r\n]*", prop_oneof![Just("\n"), Just("\r\n")]),
                1..6,
            ),
            unterminated_tail in "[^\r\n]*",
        ) {
            let mut source = String::new();
            for (body, ending) in &lines {
                source.push_str(body);
                source.push_str(ending);
            }
            source.push_str(&unterminated_tail);

            // The strategy's 1..6 size bound is the only reason this cannot fire.
            let expected = lines.last().map(|(_, ending)| *ending).expect("1..6 is non-empty");
            prop_assert_eq!(generated_line_ending(&source), expected);
        }

        /// Text appended after the last terminator never changes the answer.
        #[test]
        fn a_trailing_unterminated_line_is_inert(prefix in "(?s).*", tail in "[^\n]*") {
            let terminated = format!("{prefix}\n");
            let with_tail = format!("{terminated}{tail}");
            prop_assert_eq!(
                generated_line_ending(&terminated),
                generated_line_ending(&with_tail)
            );
        }
    }
}
