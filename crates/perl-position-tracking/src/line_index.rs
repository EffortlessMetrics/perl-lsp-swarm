//! Line indexes for UTF-8 source snapshots and UTF-16 protocol positions.
//!
//! Source lines are LF-delimited: CRLF is one separator whose LF terminates
//! the line, while bare CR and Unicode separator characters remain content.
//! This module indexes the supplied source subject without normalizing it.
use ropey::Rope;

/// Returns true if `b` is a UTF-8 continuation byte (0b10xxxxxx).
#[inline]
fn is_utf8_continuation(b: u8) -> bool {
    (b & 0b1100_0000) == 0b1000_0000
}

/// Build line starts from the shared LF-delimited source-line contract.
fn lf_line_starts(text: &str) -> Vec<usize> {
    let mut line_starts = vec![0];
    for (offset, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(offset + 1);
        }
    }
    line_starts
}

/// Build the same line starts while reading Rope chunks without using Ropey's
/// broader logical-line classification.
fn lf_line_starts_rope(rope: &Rope) -> Vec<usize> {
    let mut line_starts = vec![0];
    let mut offset = 0;
    for chunk in rope.chunks() {
        for (chunk_offset, byte) in chunk.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + chunk_offset + 1);
            }
        }
        offset += chunk.len();
    }
    line_starts
}

/// Return the end of line content before an LF or CRLF separator.
fn line_content_end(text: &str, line_start: usize, separator_end: usize) -> usize {
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

/// Rope equivalent of [`line_content_end`].
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
    /// Only LF starts a new line. CRLF is one separator and a bare CR is
    /// ordinary source content.
    pub fn new(text: &str) -> Self {
        Self { line_starts: lf_line_starts(text) }
    }

    /// Builds a cache from a [`Rope`] buffer using the same LF-delimited
    /// coordinate contract as [`Self::new`]: CRLF is one separator and its CR
    /// is not line content.
    pub fn new_rope(rope: &Rope) -> Self {
        Self { line_starts: lf_line_starts_rope(rope) }
    }

    /// Converts a byte offset in `text` to `(line, column_utf16)`.
    pub fn offset_to_position(&self, text: &str, offset: usize) -> (u32, u32) {
        let mut offset = Self::normalize_text_offset(text, offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));
        let ls = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(text.len());
        offset = offset.min(line_content_end(text, ls, separator_end));
        (line as u32, text[ls..offset].chars().map(|c| c.len_utf16()).sum::<usize>() as u32)
    }

    /// Converts `(line, column_utf16)` into a byte offset in `text`.
    pub fn position_to_offset(&self, text: &str, line: u32, character: u32) -> usize {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return text.len();
        }
        let ls = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(text.len());
        let le = line_content_end(text, ls, separator_end);
        let lt = &text[ls..le];
        let mut uc = 0;
        let mut bo = 0;
        for ch in lt.chars() {
            // Test before consuming so a column that lands inside a surrogate
            // pair clamps to the start of that codepoint rather than skipping
            // past it. `uc >= character` alone over-advances for mid-surrogate
            // requests (e.g. column 2 over "x😀y" must map to the emoji start).
            if uc + ch.len_utf16() > character as usize {
                break;
            }
            uc += ch.len_utf16();
            bo += ch.len_utf8();
        }
        ls + bo.min(lt.len())
    }

    /// Converts a byte offset in `rope` to `(line, column_utf16)`.
    pub fn offset_to_position_rope(&self, rope: &Rope, offset: usize) -> (u32, u32) {
        let mut offset = Self::normalize_rope_offset(rope, offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));
        let ls = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(rope.len_bytes());
        offset = offset.min(rope_line_content_end(rope, ls, separator_end));
        (
            line as u32,
            rope.byte_slice(ls..offset).chars().map(|c| c.len_utf16()).sum::<usize>() as u32,
        )
    }

    /// Clamp `offset` into `rope` and snap it back to a UTF-8 char boundary.
    ///
    /// `Rope::byte_slice` panics if the offset splits a multi-byte codepoint, so
    /// the clamp here mirrors [`Self::normalize_text_offset`]. Ropey 1.x does
    /// not expose `is_char_boundary` directly, so we inspect the byte at the
    /// candidate offset: UTF-8 continuation bytes always satisfy `b & 0xC0 ==
    /// 0x80` (top two bits are `10`), and every other byte is a codepoint
    /// start.
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
        let ls = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(rope.len_bytes());
        let le = rope_line_content_end(rope, ls, separator_end);
        // Clamp the slice bounds: `Rope::byte_slice` panics if `ls`/`le` exceed
        // the rope length or if `ls > le`, which a corrupt cache could trigger.
        let len = rope.len_bytes();
        let slice_start = ls.min(len);
        let slice_end = le.min(len).max(slice_start);
        let sl = rope.byte_slice(slice_start..slice_end);
        let mut uc = 0;
        let mut bo = 0;
        for ch in sl.chars() {
            // Test before consuming so a column that lands inside a surrogate
            // pair clamps to the start of that codepoint rather than skipping
            // past it. `uc >= character` alone over-advances for mid-surrogate
            // requests (e.g. column 2 over "x😀y" must map to the emoji start).
            if uc + ch.len_utf16() > character as usize {
                break;
            }
            uc += ch.len_utf16();
            bo += ch.len_utf8();
        }
        // Guard against overflow: if `ls` is corrupt/near `usize::MAX`, the add
        // would wrap to a bogus small offset. Clamp to the rope length instead.
        ls.checked_add(bo).unwrap_or(len).min(len)
    }
}

/// Stores line information for efficient position lookups, owning the text.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line start
    line_starts: Vec<usize>,
    /// The source text
    text: String,
}

impl LineIndex {
    /// Create a new LineIndex from source text
    pub fn new(text: String) -> Self {
        let line_starts = lf_line_starts(&text);
        Self { line_starts, text }
    }

    /// Borrow the source text this index owns.
    ///
    /// The index keeps a copy of the text so callers that already hold a
    /// `LineIndex` do not need to store the source a second time.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Convert byte offset to position (0-based line and UTF-16 column)
    pub fn offset_to_position(&self, offset: usize) -> (u32, u32) {
        let mut offset = self.normalize_offset(offset);
        let line = self.line_starts.binary_search(&offset).unwrap_or_else(|i| i.saturating_sub(1));

        let line_start = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(self.text.len());
        offset = offset.min(line_content_end(&self.text, line_start, separator_end));
        let column = self.utf16_column(line, offset - line_start);

        (line as u32, column as u32)
    }

    /// Convert position to byte offset
    pub fn position_to_offset(&self, line: u32, character: u32) -> Option<usize> {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return None;
        }

        let line_start = self.line_starts[line];
        let separator_end = self.line_starts.get(line + 1).copied().unwrap_or(self.text.len());
        let line_end = line_content_end(&self.text, line_start, separator_end);
        let line_text = &self.text[line_start..line_end];

        // Find the byte offset for the UTF-16 character position
        let byte_offset = self.utf16_to_byte_offset(line_text, character as usize)?;

        Some(line_start + byte_offset)
    }

    /// Get UTF-16 column from byte offset within a line
    fn utf16_column(&self, line: usize, byte_offset: usize) -> usize {
        let Some(&line_start) = self.line_starts.get(line) else {
            return 0;
        };

        // Bound the slice start: a corrupt `line_start` past the end of the
        // text (or an addition that overflows below) must not reach the slice.
        if line_start > self.text.len() {
            return 0;
        }

        // Get the text from line start to the target byte offset. Use checked
        // arithmetic so `line_start + byte_offset` cannot wrap to a value that
        // is smaller than `line_start` and produce a `start > end` slice panic.
        let Some(target_byte) = line_start.checked_add(byte_offset) else {
            return 0;
        };
        if target_byte > self.text.len() {
            return 0;
        }

        let line_text = &self.text[line_start..target_byte];

        // Count UTF-16 code units in the substring
        line_text.chars().map(|ch| ch.len_utf16()).sum()
    }

    /// Convert UTF-16 offset to byte offset within a line
    fn utf16_to_byte_offset(&self, line_text: &str, utf16_offset: usize) -> Option<usize> {
        let mut current_utf16 = 0;

        for (byte_offset, ch) in line_text.char_indices() {
            if current_utf16 == utf16_offset {
                return Some(byte_offset);
            }
            current_utf16 += ch.len_utf16();
            if current_utf16 > utf16_offset {
                // UTF-16 offset is in the middle of a character
                return None;
            }
        }

        // Check if we're at the end of the line
        if current_utf16 == utf16_offset { Some(line_text.len()) } else { None }
    }

    /// Normalize a byte offset so it is inside the text and on a UTF-8 codepoint boundary.
    fn normalize_offset(&self, offset: usize) -> usize {
        let mut normalized = offset.min(self.text.len());
        while normalized > 0 && !self.text.is_char_boundary(normalized) {
            normalized -= 1;
        }
        normalized
    }

    /// Create a range from byte offsets
    pub fn range(&self, start: usize, end: usize) -> ((u32, u32), (u32, u32)) {
        let start_pos = self.offset_to_position(start);
        let end_pos = self.offset_to_position(end);
        (start_pos, end_pos)
    }
}

#[cfg(test)]
mod overflow_hardening_tests {
    use super::*;
    use ropey::Rope;

    // Regression for #2484: a `line_start` past the end of `text` must not reach
    // the `&self.text[line_start..target_byte]` slice. Before the fix the only
    // bound check validated `target_byte`, so a corrupt `line_start` produced a
    // `start > end` slice panic.
    #[test]
    fn utf16_column_clamps_line_start_past_text_end() {
        let mut index = LineIndex::new("abc".to_string());
        // Corrupt the cache so the line start sits well beyond the text.
        index.line_starts = vec![0, 100];
        // byte_offset 0 keeps target_byte == line_start == 100 > len 3.
        assert_eq!(index.utf16_column(1, 0), 0);
    }

    // Regression for #2484: `line_start + byte_offset` must use checked
    // arithmetic. With a near-`usize::MAX` line_start the add wraps to a small
    // value that slips past the `target_byte > len` check while `line_start`
    // stays huge, slicing `[huge..small]` and panicking.
    #[test]
    fn utf16_column_does_not_overflow_on_huge_line_start() {
        let mut index = LineIndex::new("abc".to_string());
        index.line_starts = vec![0, usize::MAX];
        // usize::MAX + 5 wraps to 4 without checked arithmetic; line_start is
        // still usize::MAX, so the slice start would exceed the end.
        assert_eq!(index.utf16_column(1, 5), 0);
    }

    // Discriminator for line 240 — the `checked_add` overflow arm.  The test
    // above exercises it via the `line_start > text.len()` early-return at
    // line 232 (usize::MAX > 3), never reaching `checked_add`.  This test
    // places `line_start` exactly at `text.len()` (3 <= 3 passes the guard)
    // and then passes a `byte_offset` large enough that
    // `line_start + byte_offset` wraps past `usize::MAX`.
    #[test]
    fn utf16_column_checked_add_overflow_returns_zero() {
        let mut index = LineIndex::new("abc".to_string());
        // line_start = 3 == text.len(): passes the `> text.len()` guard.
        index.line_starts = vec![0, 3];
        // 3 + (usize::MAX - 2) = usize::MAX + 1, which overflows.
        // checked_add must catch this and return None; we must return 0.
        assert_eq!(index.utf16_column(1, usize::MAX - 2), 0);
    }

    // Discriminator for ripr seam f197bc96 — the predicate boundary
    // `line_start > self.text.len()` at line 232.
    // `utf16_column_clamps_line_start_past_text_end` (above) exercises this
    // path but ripr's static infect analysis cannot trace the private-field
    // mutation `line_starts = vec![0, 100]` to the predicate value.
    // This test names the boundary explicitly — `line_start_past_end` (100)
    // is demonstrably `> text.len()` (3) — so the discriminator is visible.
    #[test]
    fn utf16_column_boundary_discriminator() {
        let text = "abc"; // text.len() == 3
        let line_start_past_end: usize = 100; // 100 > 3 == text.len()
        debug_assert!(line_start_past_end > text.len(), "boundary: line_start > text.len()");
        let mut index = LineIndex::new(text.to_string());
        index.line_starts = vec![0, line_start_past_end];
        // The guard `if line_start > self.text.len()` at line 232 fires and
        // returns 0 before any slice occurs.
        assert_eq!(index.utf16_column(1, 0), 0);
    }

    // Regression for #2484: out-of-range `line` index must not panic on the
    // `self.line_starts[line]` access.
    #[test]
    fn utf16_column_out_of_range_line_returns_zero() {
        let index = LineIndex::new("abc".to_string());
        assert_eq!(index.utf16_column(999, 0), 0);
    }

    // Regression for #2490: a near-`usize::MAX` line start must not panic in
    // `Rope::byte_slice` and must not wrap in the final `ls + bo`. Before the
    // fix, `byte_slice(huge..len)` panicked with "start must be <= end"; after,
    // the bounds are clamped and the checked add clamps to the rope length.
    #[test]
    fn position_to_offset_rope_clamps_on_overflow() {
        let rope = Rope::from_str("hello\nworld");
        let mut cache = LineStartsCache::new_rope(&rope);
        // Corrupt the second line start to a value near usize::MAX.
        let len = cache.line_starts.len();
        cache.line_starts[len - 1] = usize::MAX - 1;
        let offset = cache.position_to_offset_rope(&rope, 1, 3);
        // Must not panic or wrap to a tiny value; clamps to the rope byte length.
        assert_eq!(offset, rope.len_bytes());
    }

    // Sanity: the hardened paths still produce correct results for valid input.
    #[test]
    fn valid_input_still_correct() {
        let index = LineIndex::new("ab\ncd".to_string());
        // Column 2 on line 0 ("ab") -> 2 UTF-16 units.
        assert_eq!(index.utf16_column(0, 2), 2);

        let rope = Rope::from_str("ab\ncd");
        let cache = LineStartsCache::new_rope(&rope);
        // Line 1 ("cd"), character 2 -> byte offset 3 (start of line 1) + 2 = 5.
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
        let owning_index = LineIndex::new(text.to_owned());

        assert_eq!(string_cache.line_starts, expected_starts);
        assert_eq!(rope_cache.line_starts, expected_starts);
        assert_eq!(owning_index.line_starts, expected_starts);
    }

    #[test]
    fn constructors_share_lf_delimited_line_starts() {
        assert_constructor_parity("", &[0]);
        assert_constructor_parity("a\nb", &[0, 2]);
        assert_constructor_parity("a\r\nb", &[0, 3]);
        assert_constructor_parity("a\rb", &[0]);
        assert_constructor_parity("a\r\nb\n", &[0, 3, 5]);
        assert_constructor_parity("a\u{000b}b\u{000c}c\u{0085}d\u{2028}e\u{2029}f", &[0]);
    }

    #[test]
    fn rope_chunks_do_not_inherit_ropey_unicode_separator_policy() {
        let text = "a\u{000b}b\u{000c}c\u{0085}d\u{2028}e\u{2029}f\ng";
        let cache = LineStartsCache::new_rope(&Rope::from_str(text));
        assert_eq!(cache.line_starts, vec![0, 17]);
    }

    #[test]
    fn bare_cr_remains_addressable_content() {
        let text = "a\rb";
        let cache = LineStartsCache::new(text);
        let index = LineIndex::new(text.to_owned());

        assert_eq!(cache.offset_to_position(text, 2), (0, 2));
        assert_eq!(index.offset_to_position(2), (0, 2));
        assert_eq!(cache.position_to_offset(text, 0, 2), 2);
        assert_eq!(index.position_to_offset(0, 2), Some(2));
    }

    #[test]
    fn crlf_separator_is_not_part_of_position_content() {
        let text = "ab\r\ncd";
        let rope = Rope::from_str(text);
        let cache = LineStartsCache::new(text);
        let rope_cache = LineStartsCache::new_rope(&rope);
        let index = LineIndex::new(text.to_owned());

        assert_eq!(cache.position_to_offset(text, 0, 2), 2);
        assert_eq!(rope_cache.position_to_offset_rope(&rope, 0, 2), 2);
        assert_eq!(index.position_to_offset(0, 2), Some(2));
        assert_eq!(index.position_to_offset(0, 3), None);
        assert_eq!(cache.position_to_offset(text, 1, 2), text.len());
        assert_eq!(rope_cache.position_to_offset_rope(&rope, 1, 2), text.len());
    }

    #[test]
    fn crlf_interior_forward_offsets_clamp_to_content_end() {
        let text = "ab\r\ncd";
        let rope = Rope::from_str(text);
        let cache = LineStartsCache::new(text);
        let rope_cache = LineStartsCache::new_rope(&rope);
        let owning_index = LineIndex::new(text.to_owned());

        assert_eq!(cache.offset_to_position(text, 3), (0, 2));
        assert_eq!(rope_cache.offset_to_position_rope(&rope, 3), (0, 2));
        assert_eq!(owning_index.offset_to_position(3), (0, 2));
    }

    #[test]
    fn leading_bom_is_preserved_by_the_generic_index() {
        let text = "\u{feff}x";
        let cache = LineStartsCache::new(text);
        let index = LineIndex::new(text.to_owned());

        assert_eq!(cache.offset_to_position(text, 3), (0, 1));
        assert_eq!(index.offset_to_position(3), (0, 1));
        assert_eq!(cache.position_to_offset(text, 0, 1), 3);
        assert_eq!(index.position_to_offset(0, 1), Some(3));
    }
}
