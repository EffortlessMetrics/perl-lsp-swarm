//! Shared rendering for an option value the launcher had to reject.
//!
//! One owner for "how a rejected value is shown back to the user", so the
//! `--port` and `--diagnostic-debounce-ms` rejections cannot drift apart on
//! either of the two ways an echoed token misleads:
//!
//! - A value carrying a control character is never written verbatim, because
//!   it would let the rejected value forge extra rendered lines in a terminal.
//! - A value padded with surrounding whitespace is quoted, because that padding
//!   is invisible exactly where it does the most damage — against the
//!   sentence punctuation. `Invalid port value:  65535.` reads as though the
//!   value had been `65535` and the server had simply failed to recognise it,
//!   so the user has no cue that whitespace is the cause.
//!
//! Rendering only. Whether a value is *rejected*, and which reason applies, is
//! still each option's own classification; this module never decides that.

/// Stand-in for a value that is not safe to write to a terminal verbatim.
pub(super) const UNRENDERABLE_TOKEN: &str = "<unprintable>";

/// Render a rejected option value so that invisible characters are visible.
///
/// A value that is already safe and unambiguous to read is returned unchanged,
/// so a token the user typed exactly as intended still renders exactly as
/// typed.
pub(super) fn render_offending_value(raw: &str) -> String {
    if raw.bytes().any(|byte| byte.is_ascii_control()) {
        return UNRENDERABLE_TOKEN.to_string();
    }
    if has_surrounding_whitespace(raw) {
        // `{:?}` supplies the delimiting quotes and escapes any embedded quote
        // or backslash, so the rendered value cannot be read as a shorter one.
        return format!("{raw:?}");
    }
    raw.to_string()
}

/// Whether a token is padded with whitespace the rendered rejection would hide.
///
/// `str::trim` covers Unicode whitespace, not just ASCII spaces, so a
/// configuration value padded with U+00A0 is caught on the same footing as one
/// padded with a plain space.
pub(super) fn has_surrounding_whitespace(raw: &str) -> bool {
    raw.trim() != raw
}

#[cfg(test)]
mod tests {
    use super::{UNRENDERABLE_TOKEN, has_surrounding_whitespace, render_offending_value};

    #[test]
    fn unambiguously_readable_values_render_unchanged() {
        for raw in ["65535", "abc", "0x10", "8080.0", "80_80", "+", "-", "-0", "-000", "1e2"] {
            assert_eq!(render_offending_value(raw), raw, "must render {raw:?} verbatim");
        }
    }

    #[test]
    fn padded_values_are_quoted_so_the_padding_is_visible() {
        assert_eq!(render_offending_value(" 65535"), "\" 65535\"");
        assert_eq!(render_offending_value("65535 "), "\"65535 \"");
        assert_eq!(render_offending_value(" 65535 "), "\" 65535 \"");
        assert_eq!(render_offending_value(" "), "\" \"");
    }

    /// Padding that renders as a space but is not a space has to be shown as
    /// something the reader can tell apart from an ASCII space. `{:?}` escapes
    /// U+00A0 to `\u{a0}`, which is strictly better than quoting it verbatim:
    /// a raw U+00A0 between two quotes still looks like the delimiter padding
    /// this change exists to reveal.
    #[test]
    fn non_ascii_padding_is_shown_as_an_escape_not_a_lookalike() {
        let rendered = render_offending_value("\u{a0}65535");
        assert_eq!(rendered, "\"\\u{a0}65535\"");
        assert!(
            !rendered.contains('\u{a0}'),
            "the lookalike must not survive into the rendering: {rendered}"
        );
    }

    /// The control-character guard outranks quoting. A tab or newline is
    /// invisible *and* can forge rendered lines, so it is replaced outright
    /// rather than quoted — quoting it would put a raw newline inside the
    /// quotes, which is the forging this guard exists to prevent.
    #[test]
    fn control_characters_outrank_quoting() {
        assert_eq!(render_offending_value("\t65535"), UNRENDERABLE_TOKEN);
        assert_eq!(render_offending_value("65535\n"), UNRENDERABLE_TOKEN);
        assert_eq!(render_offending_value("\t65535\n"), UNRENDERABLE_TOKEN);
        assert_eq!(render_offending_value(" 65535\n"), UNRENDERABLE_TOKEN);
    }

    /// A quoted value must still be a faithful, unambiguous rendering of the
    /// input: the delimiters bound it, and an embedded quote cannot terminate
    /// the rendering early and let the rest of the token pass as plain text.
    #[test]
    fn quoting_escapes_embedded_quotes_and_backslashes() {
        assert_eq!(render_offending_value(" 65\"535"), "\" 65\\\"535\"");
        assert_eq!(render_offending_value(" 65\\535"), "\" 65\\\\535\"");
    }

    #[test]
    fn control_characters_are_never_echoed_verbatim() {
        assert_eq!(render_offending_value("line\nInjected: fake"), UNRENDERABLE_TOKEN);
        assert_eq!(render_offending_value("tab\there"), UNRENDERABLE_TOKEN);
        assert_eq!(render_offending_value("\u{7}bell"), UNRENDERABLE_TOKEN);
    }

    /// Empty stays empty: it carries no padding to reveal, and both `--port=`
    /// and `--diagnostic-debounce-ms=` are reported as a missing value before
    /// they can reach a rejection.
    #[test]
    fn empty_values_are_left_alone() {
        assert_eq!(render_offending_value(""), "");
        assert!(!has_surrounding_whitespace(""));
    }

    #[test]
    fn whitespace_detection_separates_padded_from_interior() {
        assert!(has_surrounding_whitespace(" 65535"));
        assert!(has_surrounding_whitespace("65535 "));
        assert!(has_surrounding_whitespace(" 65535 "));
        assert!(has_surrounding_whitespace("  "));
        // U+00A0 is whitespace to `str::trim` and to the shell user who
        // pasted it out of a document.
        assert!(has_surrounding_whitespace("\u{a0}65535"));

        // Interior whitespace is already visible in the rendered sentence, so
        // it must not trigger quoting — `6 5535` is not a padded `65535`.
        assert!(!has_surrounding_whitespace("6 5535"));
        assert!(!has_surrounding_whitespace("65535"));
    }
}
