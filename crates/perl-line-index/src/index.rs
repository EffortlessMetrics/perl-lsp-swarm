use std::ops::Range;

/// Line index for byte <-> (line, col) mapping.
#[derive(Clone, Debug)]
pub struct LineIndex {
    pub(crate) line_starts: Vec<usize>,
    pub(crate) text_len: usize,
}

impl LineIndex {
    /// Build a line index from UTF-8 text.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts, text_len: text.len() }
    }

    pub(crate) fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    pub(crate) fn line_end_inclusive(&self, line: usize) -> Option<usize> {
        self.line_start(line).map(|_| {
            self.line_starts
                .get(line + 1)
                .map_or(self.text_len, |next_start| next_start.saturating_sub(1))
        })
    }

    pub(crate) fn line_byte_range_exclusive(&self, line: usize) -> Option<Range<usize>> {
        let start = self.line_start(line)?;
        let end = self.line_starts.get(line + 1).copied().unwrap_or(self.text_len);
        Some(start..end)
    }

    pub(crate) fn byte_column_to_offset(&self, line: usize, column: usize) -> Option<usize> {
        let start = self.line_start(line)?;
        let line_end = self.line_end_inclusive(line)?;
        let max_column = line_end.saturating_sub(start);

        if column > max_column {
            return None;
        }

        Some(start + column)
    }
}
