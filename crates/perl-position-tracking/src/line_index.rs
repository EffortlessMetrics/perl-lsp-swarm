//! Line indexes for UTF-8 source snapshots and UTF-16 protocol positions.
//!
//! The text-document newline contract is LF-delimited. CRLF is one separator
//! because its LF byte terminates the line; a bare CR remains ordinary content.
//! A decoded leading BOM remains `U+FEFF` in the indexed source.

use ropey::Rope;

/// Returns true if `b` is a UTF-8 continuation byte (`0b10xxxxxx`).
#[inline]
fn is_utf8_continuation(b: u8) -> bool {
    (b & 0b1100_0000) == 0b1000_0000
}

/// Build LF-delimited line starts without normalizing the source bytes.
fn lf_line_starts(text: &str) -> Vec<usize> {
    let mut line_starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(index + 1);
        }
    }
    line_starts
}

/// Return the line-content end for a range ending at the next line start.
fn text_line_content_end(text: &str, line_start: usize, separator_end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = separator_end.min(bytes.len()).max(line_start.min(bytes.len()));
    if end > line_start && bytes[end - 1] == b'\n' {
        end -= 1;
        if end > line_start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    }
    end
}

/// Return the Rope line-content end for a range ending at the next line start.
fn rope_line_content_end(rope: &Rope, line_start: usize, separator_end: usize) -> usize {
    let len = rope.len_bytes();
    let start = line_start.min(len);
    let mut end = separator_end.min(len).max(start);
    if end > start && rope.byte(end - 1) == b'\n' {
        end -= 1;
        if end > start && rope.byte(end - 1) == b'\r' {
            end -= 1;
        }
    }
    end
}

/// Caches byte offsets for line starts to speed up coordinate conversion.
#[derive(Debug, Clone)]
pub struct LineStartsCache {
    line_starts: Vec<usize>,
}

impl LineStartsCache {
    /// Clamp `offset` into `text` and ensure it is on a UTF-8 char boundary.
    fn normalize_text_offset(text: &str, offset: usize) -> usize {
        let mut normalized = offset.min(text.len());
        while normalized > 0 && !text.is_char_boundary(normalized) {
            normalized -= 1;
        }
        normalized
    }

    /// Builds a cache from UTF-8 source text.
    ///
    /// Lines are LF-delimited. CRLF is therefore one separator and a bare CR
    /// remains ordinary content. The source is never normalized.
    pub fn new(text: &str) -> Self {
        Self { line_starts: lf_line_starts(text) }
    }

    /// Builds a cache from a [`Rope`] buffer using the same LF-delimited model.
    pub fn new_rope(rope: &Rope) -> Self {
        let mut line_starts = vec![0];
        for line in 1..rope.len_lines() {
            line_starts.push(rope.line_to_byte(line));
        }
        Self { line_starts }
    }

    /// Converts a byte offset in `text` to `(line, column_utf16)`.
    pub fn offset_to_position(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = Self::normalize_text_offset(text, offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));
        let line_start = self.line_starts[line];
        (
            line as u32,
            text[line_start..offset].chars().map(|character| character.len_utf16()).sum::<usize>()
                as u32,
        )
    }

    /// Converts `(line, column_utf16)` into a byte offset in `text`.
    pub fn position_to_offset(&self, text: &str, line: u32, character: u32) -> usize {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return text.len();
        }
        let line_start = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(text.len());
        let line_end = text_line_content_end(text, line_start, separator_end);
        let line_text = &text[line_start..line_end];
        let mut utf16_column = 0usize;
        let mut byte_offset = 0usize;
        for character_value in line_text.chars() {
            if utf16_column + character_value.len_utf16() > character as usize {
                break;
            }
            utf16_column += character_value.len_utf16();
            byte_offset += character_value.len_utf8();
        }
        line_start + byte_offset.min(line_text.len())
    }

    /// Converts a byte offset in `rope` to `(line, column_utf16)`.
    pub fn offset_to_position_rope(&self, rope: &Rope, offset: usize) -> (u32, u32) {
        let offset = Self::normalize_rope_offset(rope, offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));
        let line_start = self.line_starts[line];
        (
            line as u32,
            rope.byte_slice(line_start..offset)
                .chars()
                .map(|character| character.len_utf16())
                .sum::<usize>() as u32,
        )
    }

    /// Clamp `offset` into `rope` and snap it back to a UTF-8 char boundary.
    fn normalize_rope_offset(rope: &Rope, offset: usize) -> usize {
        let len = rope.len_bytes();
        let mut normalized = offset.min(len);
        while normalized > 0 && normalized < len && is_utf8_continuation(rope.byte(normalized)) {
            normalized -= 1;
        }
        normalized
    }

    /// Converts `(line, column_utf16)` into a byte offset in `rope`.
    pub fn position_to_offset_rope(&self, rope: &Rope, line: u32, character: u32) -> usize {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return rope.len_bytes();
        }
        let line_start = self.line_starts[line];
        let separator_end =
            self.line_starts.get(line + 1).copied().unwrap_or_else(|| rope.len_bytes());
        let line_end = rope_line_content_end(rope, line_start, separator_end);

        let len = rope.len_bytes();
        let slice_start = line_start.min(len);
        let slice_end = line_end.min(len).max(slice_start);
        let line_slice = rope.byte_slice(slice_start..slice_end);
        let mut utf16_column = 0usize;
        let mut byte_offset = 0usize;
        for character_value in line_slice.chars() {
            if utf16_column + character_value.len_utf16() > character as usize {
                break;
            }
            utf16_column += character_value.len_utf16();
            byte_offset += character_value.len_utf8();
        }
        line_start.checked_add(byte_offset).unwrap_or(len).min(len)
    }
}

/// Stores line information for efficient position lookups, owning the text.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line start.
    line_starts: Vec<usize>,
    /// The source text.
    text: String,
}

impl LineIndex {
    /// Create a new [`LineIndex`] from source text.
    ///
    /// The index follows the same LF-delimited newline and preserved-BOM
    /// contract as [`LineStartsCache`].
    pub fn new(text: String) -> Self {
        let line_starts = lf_line_starts(&text);
        Self { line_starts, text }
    }

    /// Borrow the source text this index owns.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Convert byte offset to position (zero-based line and UTF-16 column).
    pub fn offset_to_position(&self, offset: usize) -> (u32, u32) {
        let offset = self.normalize_offset(offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));
        let line_start = self.line_starts[line];
        let column = self.utf16_column(line, offset - line_start);
        (line as u32, column as u32)
    }

    /// Convert a zero-based line and UTF-16 column to a byte offset.
    pub fn position_to_offset(&self, line: u32, character: u32) -> Option<usize> {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return None;
        }

        let line_start = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(self.text.len());
        let line_end = text_line_content_end(&self.text, line_start, separator_end);
        let line_text = &self.text[line_start..line_end];
        let byte_offset = self.utf16_to_byte_offset(line_text, character as usize)?;
        Some(line_start + byte_offset)
    }

    /// Get a UTF-16 column from a byte offset within a line.
    fn utf16_column(&self, line: usize, byte_offset: usize) -> usize {
        let Some(&line_start) = self.line_starts.get(line) else {
            return 0;
        };
        if line_start > self.text.len() {
            return 0;
        }
        let Some(target_byte) = line_start.checked_add(byte_offset) else {
            return 0;
        };
        if target_byte > self.text.len() {
            return 0;
        }
        self.text[line_start..target_byte].chars().map(|character| character.len_utf16()).sum()
    }

    /// Convert a UTF-16 offset to a byte offset within a line.
    fn utf16_to_byte_offset(&self, line_text: &str, utf16_offset: usize) -> Option<usize> {
        let mut current_utf16 = 0usize;
        for (byte_offset, character) in line_text.char_indices() {
            if current_utf16 == utf16_offset {
                return Some(byte_offset);
            }
            current_utf16 += character.len_utf16();
            if current_utf16 > utf16_offset {
                return None;
            }
        }
        (current_utf16 == utf16_offset).then_some(line_text.len())
    }

    /// Normalize a byte offset into the source and onto a UTF-8 boundary.
    fn normalize_offset(&self, offset: usize) -> usize {
        let mut normalized = offset.min(self.text.len());
        while normalized > 0 && !self.text.is_char_boundary(normalized) {
            normalized -= 1;
        }
        normalized
    }

    /// Create a position range from byte offsets.
    pub fn range(&self, start: usize, end: usize) -> ((u32, u32), (u32, u32)) {
        (self.offset_to_position(start), self.offset_to_position(end))
    }
}

#[cfg(test)]
mod overflow_hardening_tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn utf16_column_clamps_line_start_past_text_end() {
        let mut index = LineIndex::new("abc".to_string());
        index.line_starts = vec![0, 100];
        assert_eq!(index.utf16_column(1, 0), 0);
    }

    #[test]
    fn utf16_column_does_not_overflow_on_huge_line_start() {
        let mut index = LineIndex::new("abc".to_string());
        index.line_starts = vec![0, usize::MAX];
        assert_eq!(index.utf16_column(1, 5), 0);
    }

    #[test]
    fn utf16_column_checked_add_overflow_returns_zero() {
        let mut index = LineIndex::new("abc".to_string());
        index.line_starts = vec![0, 3];
        assert_eq!(index.utf16_column(1, usize::MAX - 2), 0);
    }

    #[test]
    fn utf16_column_boundary_discriminator() {
        let text = "abc";
        let line_start_past_end = 100usize;
        debug_assert!(line_start_past_end > text.len());
        let mut index = LineIndex::new(text.to_string());
        index.line_starts = vec![0, line_start_past_end];
        assert_eq!(index.utf16_column(1, 0), 0);
    }

    #[test]
    fn utf16_column_out_of_range_line_returns_zero() {
        let index = LineIndex::new("abc".to_string());
        assert_eq!(index.utf16_column(999, 0), 0);
    }

    #[test]
    fn position_to_offset_rope_clamps_on_overflow() {
        let rope = Rope::from_str("hello\nworld");
        let mut cache = LineStartsCache::new_rope(&rope);
        let len = cache.line_starts.len();
        cache.line_starts[len - 1] = usize::MAX - 1;
        assert_eq!(cache.position_to_offset_rope(&rope, 1, 3), rope.len_bytes());
    }

    #[test]
    fn valid_input_still_correct() {
        let index = LineIndex::new("ab\ncd".to_string());
        assert_eq!(index.utf16_column(0, 2), 2);

        let rope = Rope::from_str("ab\ncd");
        let cache = LineStartsCache::new_rope(&rope);
        assert_eq!(cache.position_to_offset_rope(&rope, 1, 2), 5);
    }
}

#[cfg(test)]
mod newline_policy_tests {
    use super::*;

    fn assert_constructor_parity(text: &str, expected_starts: &[usize]) {
        let string_cache = LineStartsCache::new(text);
        let rope = Rope::from_str(text);
        let rope_cache = LineStartsCache::new_rope(&rope);
        let owning_index = LineIndex::new(text.to_string());

        assert_eq!(string_cache.line_starts, expected_starts);
        assert_eq!(rope_cache.line_starts, expected_starts);
        assert_eq!(owning_index.line_starts, expected_starts);

        for line in 0..expected_starts.len() {
            let line = line as u32;
            for column in 0..=8u32 {
                assert_eq!(
                    string_cache.position_to_offset(text, line, column),
                    rope_cache.position_to_offset_rope(&rope, line, column),
                    "string/Rope mismatch for {text:?} at {line}:{column}"
                );
            }
        }
    }

    #[test]
    fn constructors_share_one_lf_delimited_contract() {
        assert_constructor_parity("", &[0]);
        assert_constructor_parity("a\nb", &[0, 2]);
        assert_constructor_parity("a\r\nb", &[0, 3]);
        assert_constructor_parity("a\rb", &[0]);
        assert_constructor_parity("a\r\nb\nc\rd", &[0, 3, 5]);
        assert_constructor_parity("\u{feff}a\n", &[0, 5]);
    }

    #[test]
    fn bare_cr_remains_addressable_content() {
        let text = "a\rb";
        let cache = LineStartsCache::new(text);
        let index = LineIndex::new(text.to_string());

        assert_eq!(cache.offset_to_position(text, 2), (0, 2));
        assert_eq!(index.offset_to_position(2), (0, 2));
        assert_eq!(cache.position_to_offset(text, 0, 2), 2);
        assert_eq!(index.position_to_offset(0, 2), Some(2));
    }

    #[test]
    fn crlf_excludes_separator_bytes_from_position_to_offset() {
        let text = "ab\r\ncd";
        let rope = Rope::from_str(text);
        let cache = LineStartsCache::new(text);
        let rope_cache = LineStartsCache::new_rope(&rope);
        let index = LineIndex::new(text.to_string());

        assert_eq!(cache.position_to_offset(text, 0, 2), 2);
        assert_eq!(rope_cache.position_to_offset_rope(&rope, 0, 2), 2);
        assert_eq!(index.position_to_offset(0, 2), Some(2));
        assert_eq!(index.position_to_offset(0, 3), None);
        assert_eq!(cache.position_to_offset(text, 1, 2), text.len());
        assert_eq!(rope_cache.position_to_offset_rope(&rope, 1, 2), text.len());
    }

    #[test]
    fn bom_is_preserved_as_source_content() {
        let text = "\u{feff}x";
        let cache = LineStartsCache::new(text);
        let index = LineIndex::new(text.to_string());

        assert_eq!(cache.offset_to_position(text, 3), (0, 1));
        assert_eq!(index.offset_to_position(3), (0, 1));
        assert_eq!(cache.position_to_offset(text, 0, 1), 3);
        assert_eq!(index.position_to_offset(0, 1), Some(3));
    }

    #[test]
    fn final_lf_creates_a_terminal_empty_line() {
        let text = "x\n";
        let cache = LineStartsCache::new(text);
        let rope = Rope::from_str(text);
        let rope_cache = LineStartsCache::new_rope(&rope);
        let index = LineIndex::new(text.to_string());

        assert_eq!(cache.line_starts, vec![0, 2]);
        assert_eq!(rope_cache.line_starts, vec![0, 2]);
        assert_eq!(index.line_starts, vec![0, 2]);
        assert_eq!(cache.position_to_offset(text, 1, 0), text.len());
        assert_eq!(rope_cache.position_to_offset_rope(&rope, 1, 0), text.len());
        assert_eq!(index.position_to_offset(1, 0), Some(text.len()));
    }
}
