//! Cursor-oriented symbol extraction for Perl source text.
//!
//! The cursor helpers share one byte-oriented lexical primitive. This keeps
//! token identity and range geometry aligned while leaving UTF-16/LSP
//! conversion to the protocol boundary.

/// Symbol sigil categories used for cursor extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorSymbolKind {
    /// Scalar variable ($foo)
    Scalar,
    /// Array variable (@foo)
    Array,
    /// Hash variable (%foo)
    Hash,
    /// Subroutine reference (&foo) or bare callable.
    Subroutine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenSpan {
    start: usize,
    end: usize,
    name_start: usize,
    kind: CursorSymbolKind,
}

#[inline]
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':'
}

#[inline]
fn is_sigil(byte: u8) -> bool {
    matches!(byte, b'$' | b'@' | b'%' | b'&' | b'*')
}

#[inline]
fn sigil_kind(byte: u8) -> CursorSymbolKind {
    match byte {
        b'$' => CursorSymbolKind::Scalar,
        b'@' => CursorSymbolKind::Array,
        b'%' => CursorSymbolKind::Hash,
        b'&' | b'*' => CursorSymbolKind::Subroutine,
        _ => CursorSymbolKind::Subroutine,
    }
}

/// Locate the complete token containing a byte anchor.
///
/// The accepted token profile is deliberately small and explicit: ASCII
/// identifier bytes, package separators, and one leading sigil. Braced
/// dereferences, Unicode identifiers, and nested sigil forms remain outside
/// this lexical layer and return None.
fn token_span(position: usize, source: &str) -> Option<TokenSpan> {
    let bytes = source.as_bytes();
    if position > bytes.len() || !source.is_char_boundary(position) {
        return None;
    }

    let anchor = if position == bytes.len() {
        let previous = position.checked_sub(1)?;
        if !is_name_byte(bytes[previous]) && !is_sigil(bytes[previous]) {
            return None;
        }
        previous
    } else if is_name_byte(bytes[position]) || is_sigil(bytes[position]) {
        position
    } else if position > 0
        && bytes[position].is_ascii_whitespace()
        && (is_name_byte(bytes[position - 1]) || is_sigil(bytes[position - 1]))
    {
        // A byte offset at the governed just-after-token boundary is valid.
        position - 1
    } else {
        return None;
    };

    let mut start = anchor;
    if !is_sigil(bytes[anchor]) {
        while start > 0 && is_name_byte(bytes[start - 1]) {
            start -= 1;
        }
    }
    if start > 0 && is_sigil(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = anchor + 1;
    while end < bytes.len() && is_name_byte(bytes[end]) {
        end += 1;
    }

    while end > start && bytes[end - 1] == b':' {
        end -= 1;
    }
    if end <= start {
        return None;
    }

    let (name_start, kind) = if is_sigil(bytes[start]) {
        (start + 1, sigil_kind(bytes[start]))
    } else {
        (start, CursorSymbolKind::Subroutine)
    };

    if name_start >= end
        || (!bytes[name_start].is_ascii_alphanumeric() && bytes[name_start] != b'_')
    {
        return None;
    }

    Some(TokenSpan { start, end, name_start, kind })
}

/// Extract a symbol and its kind from source at position.
pub fn extract_symbol_from_source(
    position: usize,
    source: &str,
) -> Option<(String, CursorSymbolKind)> {
    let span = token_span(position, source)?;
    Some((source[span.name_start..span.end].to_string(), span.kind))
}

/// Get symbol range at position, including a leading sigil.
///
/// The returned pair is a non-empty half-open byte range. A cursor anywhere
/// within the token, or at the governed whitespace/end-of-source boundary,
/// returns the same range.
pub fn get_symbol_range_at_position(position: usize, source: &str) -> Option<(usize, usize)> {
    let span = token_span(position, source)?;
    Some((span.start, span.end))
}

/// Return true when byte is a module/name character ([A-Za-z0-9_:]).
#[inline]
pub fn is_modchar(byte: u8) -> bool {
    is_name_byte(byte)
}

/// Convert a UTF-16 column index to a byte offset for a single line.
#[inline]
pub fn byte_offset_utf16(line_text: &str, col_utf16: usize) -> usize {
    let mut units = 0;
    for (index, ch) in line_text.char_indices() {
        if units >= col_utf16 {
            return index;
        }
        units += if ch as u32 >= 0x10000 { 2 } else { 1 };
        if units > col_utf16 {
            return index;
        }
    }
    line_text.len()
}

/// Extract the module/symbol token under the cursor (UTF-16 aware).
pub fn token_under_cursor(text: &str, line: usize, col_utf16: usize) -> Option<String> {
    let line_text = text.lines().nth(line)?;
    let byte_position = byte_offset_utf16(line_text, col_utf16);
    let span = token_span(byte_position, line_text)?;
    Some(line_text[span.start..span.end].to_string())
}

/// Check if a match at pos..pos+word_len is bounded by non-word chars.
pub fn is_word_boundary(text: &[u8], pos: usize, word_len: usize) -> bool {
    let Some(end_pos) = pos.checked_add(word_len) else {
        return false;
    };
    if pos > 0 && is_modchar(text[pos - 1]) {
        return false;
    }
    end_pos >= text.len() || !is_modchar(text[end_pos])
}

#[cfg(test)]
mod tests {
    use super::{
        CursorSymbolKind, byte_offset_utf16, extract_symbol_from_source,
        get_symbol_range_at_position, is_modchar, is_word_boundary, token_under_cursor,
    };

    #[test]
    fn all_helpers_share_complete_middle_token_identity() {
        let source = "my $my_func = Demo::Worker;";
        for position in 4..11 {
            assert_eq!(
                extract_symbol_from_source(position, source),
                Some(("my_func".to_string(), CursorSymbolKind::Scalar))
            );
            assert_eq!(get_symbol_range_at_position(position, source), Some((3, 11)));
        }
        assert_eq!(token_under_cursor(source, 0, 7), Some("$my_func".to_string()));
    }

    #[test]
    fn qualified_names_and_bare_subroutines_share_the_same_range() {
        let source = "Demo::Worker";
        for position in 0..source.len() {
            assert_eq!(get_symbol_range_at_position(position, source), Some((0, source.len())));
        }
        assert_eq!(
            extract_symbol_from_source(6, source),
            Some(("Demo::Worker".to_string(), CursorSymbolKind::Subroutine))
        );
    }

    #[test]
    fn sigils_are_included_in_ranges_but_not_names() {
        for (source, kind) in [
            ("$value", CursorSymbolKind::Scalar),
            ("@items", CursorSymbolKind::Array),
            ("%options", CursorSymbolKind::Hash),
            ("&callback", CursorSymbolKind::Subroutine),
            ("*glob", CursorSymbolKind::Subroutine),
        ] {
            let range = get_symbol_range_at_position(2.min(source.len() - 1), source);
            assert_eq!(range, Some((0, source.len())));
            assert_eq!(
                extract_symbol_from_source(1, source),
                Some((source[1..].to_string(), kind))
            );
        }
    }

    #[test]
    fn whitespace_boundary_is_supported_but_punctuation_is_not() {
        assert_eq!(get_symbol_range_at_position(3, "foo bar"), Some((0, 3)));
        assert_eq!(get_symbol_range_at_position(3, "foo;"), None);
        assert_eq!(get_symbol_range_at_position(3, "foo.bar"), None);
        assert_eq!(get_symbol_range_at_position(0, " "), None);
    }

    #[test]
    fn invalid_utf8_boundary_is_rejected() {
        let source = "😀foo";
        assert_eq!(get_symbol_range_at_position(1, source), None);
        assert_eq!(get_symbol_range_at_position(4, source), Some((4, source.len())));
    }

    #[test]
    fn unsupported_braced_and_nested_sigil_forms_are_explicitly_rejected() {
        assert!(extract_symbol_from_source(1, concat!("$", "{foo}")).is_none());
        assert!(extract_symbol_from_source(1, "$$foo").is_none());
        assert!(extract_symbol_from_source(1, concat!("$", "{^MATCH}")).is_none());
    }

    #[test]
    fn token_under_cursor_handles_utf16_and_end_boundaries() {
        let source = "😀 Demo::Worker";
        assert_eq!(token_under_cursor(source, 0, 5), Some("Demo::Worker".to_string()));
        assert_eq!(token_under_cursor("use Demo::Worker", 0, 16), Some("Demo::Worker".to_string()));
        assert_eq!(token_under_cursor("my $value = 1;", 0, 11), None);
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
    fn word_boundary_is_checked_without_overflow() {
        assert!(is_word_boundary(b" Demo::Worker ", 1, "Demo::Worker".len()));
        assert!(!is_word_boundary(b"foobar", 0, 3));
        assert!(!is_word_boundary(b"x", usize::MAX, 1));
    }

    #[test]
    fn modchar_profile_is_explicit() {
        assert!(is_modchar(b':'));
        assert!(is_modchar(b'_'));
        assert!(!is_modchar(b'$'));
        assert!(!is_modchar(b' '));
        assert!(!is_modchar(0xff));
    }
}
