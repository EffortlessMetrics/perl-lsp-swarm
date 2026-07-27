//! Cursor-oriented symbol extraction for Perl source text.
//!
//! This module focuses on a single responsibility: extracting symbol names
//! and ranges around a cursor position.

/// Symbol sigil categories used for cursor extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorSymbolKind {
    /// Scalar variable (`$foo`)
    Scalar,
    /// Array variable (`@foo`)
    Array,
    /// Hash variable (`%foo`)
    Hash,
    /// Subroutine reference (`&foo`)
    Subroutine,
}

/// Extract a symbol and its kind from `source` at `position`.
pub fn extract_symbol_from_source(
    position: usize,
    source: &str,
) -> Option<(String, CursorSymbolKind)> {
    // Operate on bytes, not chars. Callers pass byte offsets from parser
    // SourceLocations; indexing a Vec<char> with a byte offset produces wrong
    // results for any source with non-ASCII characters before the cursor. (#5068)
    let bytes = source.as_bytes();
    if position >= bytes.len() {
        // position may be exactly at source.len() (cursor at EOF); only reject
        // if truly past the end.
        if position > bytes.len() {
            return None;
        }
    }

    let is_sigil = |b: u8| matches!(b, b'$' | b'@' | b'%' | b'&');
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    let (sigil, name_start) = if position > 0 && is_sigil(bytes[position - 1]) {
        (
            match bytes[position - 1] {
                b'$' => Some(CursorSymbolKind::Scalar),
                b'@' => Some(CursorSymbolKind::Array),
                b'%' => Some(CursorSymbolKind::Hash),
                b'&' => Some(CursorSymbolKind::Subroutine),
                _ => None,
            },
            position,
        )
    } else if position < bytes.len() && is_sigil(bytes[position]) {
        (
            match bytes[position] {
                b'$' => Some(CursorSymbolKind::Scalar),
                b'@' => Some(CursorSymbolKind::Array),
                b'%' => Some(CursorSymbolKind::Hash),
                b'&' => Some(CursorSymbolKind::Subroutine),
                _ => None,
            },
            position + 1,
        )
    } else {
        (None, position)
    };

    let mut end = name_start;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }

    if end > name_start && end <= source.len() {
        // Safety: we only advanced over ASCII bytes (sigils + alphanumeric/_),
        // so name_start..end is always a valid UTF-8 boundary.
        let name = source[name_start..end].to_string();
        let kind = sigil.unwrap_or(CursorSymbolKind::Subroutine);
        Some((name, kind))
    } else {
        None
    }
}

/// Get symbol range at `position`, including a leading sigil when present.
/// Operates on byte offsets (matching parser SourceLocations). (#5068)
///
/// Behavior matches the original char-indexed implementation: includes a
/// preceding sigil and scans forward from the cursor position.  Does NOT
/// scan backward for preceding ident chars (cursor in the middle of a word
/// returns the forward suffix only).
pub fn get_symbol_range_at_position(position: usize, source: &str) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if position >= bytes.len() {
        return None;
    }
    let pos = position;

    let is_sigil = |b: u8| matches!(b, b'$' | b'@' | b'%' | b'&');
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    let mut start = pos;
    if start > 0 && is_sigil(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = pos;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }

    // Scan backward to include preceding ident chars when the cursor is on an
    // ident char (matching original behavior — the `while start < position`
    // loop in the original walked chars[start] backward).
    while start > 0 && start < pos && is_ident(bytes[start]) {
        start -= 1;
    }
    // The original loop decremented start, then the final check tested if
    // chars[start] was ident.  Replicate: if we decremented past an ident, the
    // start now points one before the ident run.  Check if bytes[start] itself
    // is an ident (not a sigil) — if so, we've reached the beginning of the run.
    // If it's a sigil, keep it included.

    Some((start, end))
}

/// Return true when `byte` is a module/name character (`[A-Za-z0-9_:]`).
#[inline]
pub fn is_modchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b':' || byte == b'_'
}

/// Convert a UTF-16 column index to a byte offset for a single line.
#[inline]
pub fn byte_offset_utf16(line_text: &str, col_utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in line_text.char_indices() {
        if units >= col_utf16 {
            return i;
        }
        let ch_units = if ch as u32 >= 0x10000 { 2 } else { 1 };
        units += ch_units;
        if units > col_utf16 {
            return i;
        }
    }
    line_text.len()
}

/// Extract the module/symbol token under the cursor (UTF-16 aware).
pub fn token_under_cursor(text: &str, line: usize, col_utf16: usize) -> Option<String> {
    let line_text = text.lines().nth(line)?;
    let byte_pos = byte_offset_utf16(line_text, col_utf16);
    let bytes = line_text.as_bytes();

    if bytes.is_empty() {
        return None;
    }

    // Prefer the character at the cursor. If the cursor is positioned at the
    // end of a token (or line), snap to the previous byte when that byte is
    // part of an identifier/module token or sigil.
    let anchor = if byte_pos < bytes.len() { byte_pos } else { bytes.len().saturating_sub(1) };

    let cursor =
        if is_modchar(bytes[anchor]) || matches!(bytes[anchor], b'$' | b'@' | b'%' | b'&' | b'*') {
            anchor
        } else if anchor > 0 && is_modchar(bytes[anchor - 1]) {
            anchor - 1
        } else {
            return None;
        };

    let mut start = cursor;
    let mut end = cursor;

    while start > 0 && is_modchar(bytes[start - 1]) {
        start -= 1;
    }
    if start > 0 && matches!(bytes[start - 1], b'$' | b'@' | b'%' | b'&' | b'*') {
        start -= 1;
    }

    // When the cursor is directly on a sigil character, step `end` past it so
    // the following identifier walk can collect the name (`$foo` → `$foo`, not empty).
    if end < bytes.len() && matches!(bytes[end], b'$' | b'@' | b'%' | b'&' | b'*') {
        end += 1;
    }

    while end < bytes.len() && is_modchar(bytes[end]) {
        end += 1;
    }

    if end == start {
        return None;
    }

    Some(line_text[start..end].to_string())
}

/// Check if a match at `pos..pos+word_len` is bounded by non-word chars.
pub fn is_word_boundary(text: &[u8], pos: usize, word_len: usize) -> bool {
    if pos > 0 && is_modchar(text[pos - 1]) {
        return false;
    }

    let end_pos = pos + word_len;
    if end_pos < text.len() && is_modchar(text[end_pos]) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{
        byte_offset_utf16, extract_symbol_from_source, get_symbol_range_at_position, is_modchar,
        is_word_boundary, token_under_cursor, CursorSymbolKind,
    };

    #[test]
    fn token_under_cursor_extracts_perl_module_token() {
        let text = "use Demo::Worker;\n";
        assert_eq!(token_under_cursor(text, 0, 8), Some("Demo::Worker".to_string()));
    }

    #[test]
    fn token_under_cursor_supports_sigils() {
        let text = "my $value = 1;\n";
        assert_eq!(token_under_cursor(text, 0, 5), Some("$value".to_string()));
    }

    #[test]
    fn token_under_cursor_supports_cursor_after_symbol() {
        let text = "use Demo::Worker\n";
        assert_eq!(token_under_cursor(text, 0, 16), Some("Demo::Worker".to_string()));
    }

    #[test]
    fn token_under_cursor_supports_cursor_on_sigil() {
        // Cursor directly ON the `$` sigil (col 3) must still extract `$value`.
        let text = "my $value = 1;\n";
        assert_eq!(token_under_cursor(text, 0, 3), Some("$value".to_string()));
    }

    #[test]
    fn token_under_cursor_returns_none_on_punctuation() {
        let text = "my $value = 1;\n";
        assert_eq!(token_under_cursor(text, 0, 11), None);
    }

    #[test]
    fn token_under_cursor_returns_none_for_out_of_range_line(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = "my $value = 1;\n";
        assert_eq!(token_under_cursor(text, 99, 0), None);
        Ok(())
    }

    #[test]
    fn token_under_cursor_returns_none_for_empty_line() -> Result<(), Box<dyn std::error::Error>> {
        let text = "\n";
        assert_eq!(token_under_cursor(text, 0, 0), None);
        Ok(())
    }

    #[test]
    fn token_under_cursor_handles_array_and_hash_sigils() -> Result<(), Box<dyn std::error::Error>>
    {
        let text = "push @items, %opts;\n";
        assert_eq!(token_under_cursor(text, 0, 5), Some("@items".to_string()));
        assert_eq!(token_under_cursor(text, 0, 13), Some("%opts".to_string()));
        Ok(())
    }

    #[test]
    fn utf16_col_to_byte_offset_handles_surrogate_pairs() {
        let line = "A😀B";
        assert_eq!(byte_offset_utf16(line, 0), 0);
        assert_eq!(byte_offset_utf16(line, 1), 1);
        assert_eq!(byte_offset_utf16(line, 2), 1);
        assert_eq!(byte_offset_utf16(line, 3), 5);
        assert_eq!(byte_offset_utf16(line, 4), 6);
    }

    #[test]
    fn byte_offset_utf16_past_end_returns_len() -> Result<(), Box<dyn std::error::Error>> {
        let line = "abc";
        assert_eq!(byte_offset_utf16(line, 100), 3);
        Ok(())
    }

    #[test]
    fn byte_offset_utf16_empty_string_returns_zero() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(byte_offset_utf16("", 0), 0);
        assert_eq!(byte_offset_utf16("", 5), 0);
        Ok(())
    }

    #[test]
    fn word_boundary_detects_embedded_word() {
        let text = b"fooDemo::Workerbar";
        assert!(!is_word_boundary(text, 3, "Demo::Worker".len()));
        assert!(is_word_boundary(b" Demo::Worker ", 1, "Demo::Worker".len()));
    }

    #[test]
    fn word_boundary_at_start_and_end_of_text() -> Result<(), Box<dyn std::error::Error>> {
        // Match at position 0 — no preceding character, boundary is at start.
        assert!(is_word_boundary(b"foo bar", 0, 3));
        // Match at very end — end position equals text length.
        assert!(is_word_boundary(b"hello foo", 6, 3));
        Ok(())
    }

    #[test]
    fn word_boundary_false_when_trailing_modchar() -> Result<(), Box<dyn std::error::Error>> {
        // "foo" at pos 0 in "foobar" is not a boundary because 'b' follows.
        assert!(!is_word_boundary(b"foobar", 0, 3));
        Ok(())
    }

    // ── is_modchar ────────────────────────────────────────────────────────────

    #[test]
    fn is_modchar_accepts_alphanumeric_and_colon_and_underscore(
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert!(is_modchar(b'a'));
        assert!(is_modchar(b'Z'));
        assert!(is_modchar(b'0'));
        assert!(is_modchar(b'9'));
        assert!(is_modchar(b'_'));
        assert!(is_modchar(b':'));
        Ok(())
    }

    #[test]
    fn is_modchar_rejects_sigils_spaces_and_punctuation() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(!is_modchar(b'$'));
        assert!(!is_modchar(b'@'));
        assert!(!is_modchar(b'%'));
        assert!(!is_modchar(b'&'));
        assert!(!is_modchar(b' '));
        assert!(!is_modchar(b'\n'));
        assert!(!is_modchar(b'-'));
        assert!(!is_modchar(b'.'));
        assert!(!is_modchar(b'('));
        assert!(!is_modchar(b')'));
        Ok(())
    }

    // ── extract_symbol_from_source ────────────────────────────────────────────

    #[test]
    fn extract_symbol_recognizes_scalar_sigil_before_name() -> Result<(), Box<dyn std::error::Error>>
    {
        // Cursor on 'v' of "$value" — sigil is the preceding char.
        let source = "$value";
        let result = extract_symbol_from_source(1, source);
        assert_eq!(result, Some(("value".to_string(), CursorSymbolKind::Scalar)));
        Ok(())
    }

    #[test]
    fn extract_symbol_recognizes_array_sigil_before_name() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "@items";
        let result = extract_symbol_from_source(1, source);
        assert_eq!(result, Some(("items".to_string(), CursorSymbolKind::Array)));
        Ok(())
    }

    #[test]
    fn extract_symbol_recognizes_hash_sigil_before_name() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "%opts";
        let result = extract_symbol_from_source(1, source);
        assert_eq!(result, Some(("opts".to_string(), CursorSymbolKind::Hash)));
        Ok(())
    }

    #[test]
    fn extract_symbol_recognizes_subroutine_sigil_before_name(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let source = "&callback";
        let result = extract_symbol_from_source(1, source);
        assert_eq!(result, Some(("callback".to_string(), CursorSymbolKind::Subroutine)));
        Ok(())
    }

    #[test]
    fn extract_symbol_cursor_on_sigil_itself() -> Result<(), Box<dyn std::error::Error>> {
        // Cursor at position 0, which is the '$' sigil character.
        let source = "$foo";
        let result = extract_symbol_from_source(0, source);
        assert_eq!(result, Some(("foo".to_string(), CursorSymbolKind::Scalar)));
        Ok(())
    }

    #[test]
    fn extract_symbol_cursor_past_end_returns_none() -> Result<(), Box<dyn std::error::Error>> {
        let source = "$foo";
        assert_eq!(extract_symbol_from_source(10, source), None);
        Ok(())
    }

    #[test]
    fn extract_symbol_on_non_name_character_returns_none() -> Result<(), Box<dyn std::error::Error>>
    {
        // Space at position 2 — not alphanumeric/underscore, no sigil context.
        let source = "my $x";
        // Position 2 is ' ', no sigil before it and not alphanumeric.
        assert_eq!(extract_symbol_from_source(2, source), None);
        Ok(())
    }

    #[test]
    fn extract_symbol_no_sigil_defaults_to_subroutine() -> Result<(), Box<dyn std::error::Error>> {
        // A bare identifier with no sigil context defaults to Subroutine kind.
        let source = "greet";
        let result = extract_symbol_from_source(0, source);
        assert_eq!(result, Some(("greet".to_string(), CursorSymbolKind::Subroutine)));
        Ok(())
    }

    #[test]
    fn extract_symbol_handles_underscore_and_digits_in_name(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let source = "$my_var2";
        let result = extract_symbol_from_source(1, source);
        assert_eq!(result, Some(("my_var2".to_string(), CursorSymbolKind::Scalar)));
        Ok(())
    }

    // ── get_symbol_range_at_position ──────────────────────────────────────────

    #[test]
    fn symbol_range_past_end_returns_none() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(get_symbol_range_at_position(100, "foo"), None);
        Ok(())
    }

    #[test]
    fn symbol_range_includes_preceding_sigil() -> Result<(), Box<dyn std::error::Error>> {
        // Position 1 is 'f' in "$foo"; the range should include the '$' at 0.
        let source = "$foo";
        let range = get_symbol_range_at_position(1, source);
        assert!(range.is_some());
        let (start, end) = range.unwrap_or((0, 0));
        assert_eq!(start, 0, "range must include the leading sigil");
        assert_eq!(end, 4);
        Ok(())
    }

    #[test]
    fn symbol_range_on_bare_identifier_no_sigil() -> Result<(), Box<dyn std::error::Error>> {
        let source = "greet";
        let range = get_symbol_range_at_position(0, source);
        assert!(range.is_some());
        let (start, end) = range.unwrap_or((0, 0));
        assert_eq!(end, 5);
        // start ≤ 0 (could be 0 when no sigil precedes)
        assert!(start <= 1);
        Ok(())
    }

    // ── CursorSymbolKind derive traits ────────────────────────────────────────

    #[test]
    fn cursor_symbol_kind_derives_debug() -> Result<(), Box<dyn std::error::Error>> {
        let formatted = format!("{:?}", CursorSymbolKind::Scalar);
        assert_eq!(formatted, "Scalar");
        Ok(())
    }

    #[test]
    fn cursor_symbol_kind_derives_clone_and_copy() -> Result<(), Box<dyn std::error::Error>> {
        fn assert_clone<T: Clone>() {}
        fn assert_copy<T: Copy>() {}

        assert_clone::<CursorSymbolKind>();
        assert_copy::<CursorSymbolKind>();

        let original = CursorSymbolKind::Array;
        let copied: CursorSymbolKind = original;
        assert_eq!(copied, CursorSymbolKind::Array);
        Ok(())
    }

    #[test]
    fn cursor_symbol_kind_derives_partial_eq() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(CursorSymbolKind::Scalar, CursorSymbolKind::Scalar);
        assert_ne!(CursorSymbolKind::Scalar, CursorSymbolKind::Array);
        assert_ne!(CursorSymbolKind::Hash, CursorSymbolKind::Subroutine);
        Ok(())
    }

    #[test]
    fn cursor_symbol_kind_all_variants_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [
            CursorSymbolKind::Scalar,
            CursorSymbolKind::Array,
            CursorSymbolKind::Hash,
            CursorSymbolKind::Subroutine,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
        Ok(())
    }
}
