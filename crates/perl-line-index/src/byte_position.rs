use crate::LineIndex;

impl LineIndex {
    /// Convert a byte offset to `(line, column)` using byte columns.
    #[must_use]
    pub fn byte_to_position(&self, byte: usize) -> (usize, usize) {
        let line = self.line_starts.binary_search(&byte).unwrap_or_else(|i| i.saturating_sub(1));
        let column = byte - self.line_starts[line];
        (line, column)
    }

    /// Convert `(line, column)` back to byte offset.
    ///
    /// Returns `None` when the line is out of range or when the column extends
    /// past the end of the line (including the newline character, but not the
    /// start of the next line).
    #[must_use]
    pub fn position_to_byte(&self, line: usize, column: usize) -> Option<usize> {
        self.byte_column_to_offset(line, column)
    }

    /// Convert `(line, column)` back to byte offset, returning `None` when
    /// the column crosses the line boundary.
    ///
    /// The newline character at the end of a line is the last addressable
    /// column on that line.  The byte at `next_line_start` belongs to the
    /// *next* line and is therefore out of range.
    #[must_use]
    pub fn position_to_byte_checked(&self, line: usize, column: usize) -> Option<usize> {
        self.byte_column_to_offset(line, column)
    }
}
