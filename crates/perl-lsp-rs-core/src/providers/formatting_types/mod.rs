//! Shared formatting types for Perl LSP integrations.

#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// Text edit for formatting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatTextEdit {
    /// The range to replace.
    pub range: FormatRange,
    /// The new text.
    #[serde(rename = "newText")]
    pub new_text: String,
}

/// Position in a document (UTF-16 based).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatPosition {
    /// Line position (0-based).
    pub line: u32,
    /// Character position (UTF-16, 0-based).
    pub character: u32,
}

impl FormatPosition {
    /// Create a new position.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Range in a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatRange {
    /// Start position.
    pub start: FormatPosition,
    /// End position.
    pub end: FormatPosition,
}

fn true_eof_position(content: &str) -> FormatPosition {
    let mut line = 0_u32;
    let mut character = 0_u32;
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                line = line.saturating_add(1);
                character = 0;
            }
            '\n' => {
                line = line.saturating_add(1);
                character = 0;
            }
            _ => {
                let width = if ch as u32 >= 0x10000 { 2 } else { 1 };
                character = character.saturating_add(width);
            }
        }
    }

    FormatPosition { line, character }
}

impl FormatRange {
    /// Create a range covering the entire document through its true EOF.
    ///
    /// A terminal line separator creates a final empty line, so the end of
    /// `"text\n"`, `"text\r\n"`, and supported bare-CR `"text\r"` is
    /// `(1, 0)`, not the end of line zero. CRLF is treated as one separator and
    /// non-BMP characters count as two UTF-16 code units.
    pub fn whole_document(content: &str) -> Self {
        Self { start: FormatPosition { line: 0, character: 0 }, end: true_eof_position(content) }
    }

    /// Create a new range from positions.
    pub fn new(start: FormatPosition, end: FormatPosition) -> Self {
        Self { start, end }
    }
}

/// Formatting options.
#[derive(Debug, Clone, Deserialize)]
pub struct FormattingOptions {
    /// Size of a tab in spaces.
    #[serde(rename = "tabSize")]
    pub tab_size: u32,
    /// Prefer spaces over tabs.
    #[serde(rename = "insertSpaces")]
    pub insert_spaces: bool,
    /// Trim trailing whitespace on a line.
    #[serde(rename = "trimTrailingWhitespace")]
    pub trim_trailing_whitespace: Option<bool>,
    /// Insert a newline character at the end of the file.
    #[serde(rename = "insertFinalNewline")]
    pub insert_final_newline: Option<bool>,
    /// Trim all newlines after the final newline at the end of the file.
    #[serde(rename = "trimFinalNewlines")]
    pub trim_final_newlines: Option<bool>,
}

/// Formatted document result.
#[derive(Debug, Clone, Default)]
pub struct FormattedDocument {
    /// The formatted text.
    pub text: String,
    /// Text edits to apply formatting.
    pub edits: Vec<FormatTextEdit>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_whole_document_end(content: &str, line: u32, character: u32) {
        let range = FormatRange::whole_document(content);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, line);
        assert_eq!(range.end.character, character);
    }

    #[test]
    fn test_formatting_options() {
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(true),
            trim_final_newlines: Some(true),
        };

        assert_eq!(options.tab_size, 4);
        assert!(options.insert_spaces);
    }

    #[test]
    fn test_format_position() {
        let position = FormatPosition::new(5, 10);
        assert_eq!(position.line, 5);
        assert_eq!(position.character, 10);
    }

    #[test]
    fn test_format_range() {
        let start = FormatPosition::new(0, 0);
        let end = FormatPosition::new(10, 20);
        let range = FormatRange::new(start, end);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.end.line, 10);
    }

    #[test]
    fn whole_document_reaches_empty_and_unterminated_eof() {
        assert_whole_document_end("", 0, 0);
        assert_whole_document_end("abc", 0, 3);
        assert_whole_document_end("a\nb", 1, 1);
    }

    #[test]
    fn whole_document_preserves_terminal_line_identity() {
        assert_whole_document_end("a\n", 1, 0);
        assert_whole_document_end("a\r\n", 1, 0);
        assert_whole_document_end("a\r", 1, 0);
        assert_whole_document_end("a\r\n\r\nb", 2, 1);
    }

    #[test]
    fn whole_document_counts_utf16_without_splitting_crlf() {
        assert_whole_document_end("😀", 0, 2);
        assert_whole_document_end("a\r\nb😀", 1, 3);
        assert_whole_document_end("😀\n", 1, 0);
    }
}
