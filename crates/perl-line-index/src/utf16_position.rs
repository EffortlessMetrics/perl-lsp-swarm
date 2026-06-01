use crate::LineIndex;

impl LineIndex {
    /// Convert `(line, column)` back to byte offset where `column` is a
    /// **UTF-16 code unit** offset (the unit used by the LSP `Position.character`
    /// field).
    ///
    /// This is the LSP-safe counterpart to [`position_to_byte`], which uses
    /// raw byte columns.  On lines that contain only ASCII, the two functions
    /// return identical results.  On lines with multibyte UTF-8 characters the
    /// byte offset can be larger than the UTF-16 column number.
    ///
    /// # Parameters
    ///
    /// - `text` — the original UTF-8 source text that was used to build this
    ///   index.  The caller must pass the same text that was given to [`new`].
    /// - `line` — 0-based line number.
    /// - `column` — 0-based UTF-16 code unit offset from the start of the line.
    ///
    /// # Return value
    ///
    /// Returns `None` when:
    /// - `line` is out of range (same as [`position_to_byte`]).
    /// - `column` is past the end of the line (UTF-16 length of the line text).
    /// - `column` points into the middle of a UTF-16 surrogate pair (the
    ///   interior of a supplementary character, U+10000..=U+10FFFF).
    ///
    /// The returned byte offset is always on a UTF-8 character boundary.
    ///
    /// [`new`]: Self::new
    /// [`position_to_byte`]: Self::position_to_byte
    #[must_use]
    pub fn position_to_byte_utf16(&self, text: &str, line: usize, column: usize) -> Option<usize> {
        let line_range = self.line_byte_range_exclusive(line)?;
        let line_text = text.get(line_range.clone())?;
        utf16_column_to_byte_offset(line_text, column)
            .map(|byte_offset| line_range.start + byte_offset)
    }
}

fn utf16_column_to_byte_offset(line_text: &str, column: usize) -> Option<usize> {
    let mut utf16_units: usize = 0;
    for (byte_offset, ch) in line_text.char_indices() {
        if utf16_units == column {
            return Some(byte_offset);
        }
        let ch_utf16 = ch.len_utf16();
        if utf16_units + ch_utf16 > column {
            return None;
        }
        utf16_units += ch_utf16;
    }

    if utf16_units == column { Some(line_text.len()) } else { None }
}
