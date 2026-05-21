use ropey::Rope;

pub(super) fn lsp_pos_to_byte(rope: &Rope, line: usize, character: usize) -> usize {
    let Some(line_ctx) = LineContext::new(rope, line) else {
        return rope.len_bytes();
    };

    line_ctx.byte_offset_for_utf16_col(character)
}

pub(super) fn byte_to_lsp_pos(rope: &Rope, byte_offset: usize) -> (usize, usize) {
    let bounded_offset = byte_offset.min(rope.len_bytes());
    let line = rope.byte_to_line(bounded_offset);
    let line_start = rope.line_to_byte(line);
    let column_bytes = bounded_offset - line_start;

    let utf16_column = utf16_col_for_byte_offset(rope.line(line), column_bytes);

    (line, utf16_column)
}

struct LineContext<'a> {
    line_start: usize,
    line_text: ropey::RopeSlice<'a>,
}

impl<'a> LineContext<'a> {
    fn new(rope: &'a Rope, line: usize) -> Option<Self> {
        if line >= rope.len_lines() {
            return None;
        }

        Some(Self {
            line_start: rope.line_to_byte(line),
            line_text: rope.line(line),
        })
    }

    fn byte_offset_for_utf16_col(&self, utf16_target: usize) -> usize {
        self.line_start + byte_offset_for_utf16_col(self.line_text, utf16_target)
    }
}

fn byte_offset_for_utf16_col(line: ropey::RopeSlice<'_>, utf16_target: usize) -> usize {
    let mut utf16_pos = 0;
    let mut byte_pos = 0;

    for ch in line.chars() {
        if utf16_pos >= utf16_target {
            break;
        }
        utf16_pos += ch.len_utf16();
        byte_pos += ch.len_utf8();
    }

    byte_pos
}

fn utf16_col_for_byte_offset(line: ropey::RopeSlice<'_>, target_bytes: usize) -> usize {
    let mut utf16_pos = 0;
    let mut current_bytes = 0;

    for ch in line.chars() {
        if current_bytes >= target_bytes {
            break;
        }
        current_bytes += ch.len_utf8();
        utf16_pos += ch.len_utf16();
    }

    utf16_pos
}

#[cfg(test)]
mod tests {
    use super::{byte_to_lsp_pos, lsp_pos_to_byte};
    use ropey::Rope;

    #[test]
    fn lsp_pos_to_byte_roundtrips_line_positions() {
        let text = "Hello\nWorld\n";
        let rope = Rope::from_str(text);

        assert_eq!(lsp_pos_to_byte(&rope, 0, 0), 0);
        assert_eq!(lsp_pos_to_byte(&rope, 1, 0), 6);
        assert_eq!(lsp_pos_to_byte(&rope, 1, 3), 9);
    }

    #[test]
    fn byte_to_lsp_pos_roundtrips_line_positions() {
        let text = "Hello\nWorld\n";
        let rope = Rope::from_str(text);

        assert_eq!(byte_to_lsp_pos(&rope, 0), (0, 0));
        assert_eq!(byte_to_lsp_pos(&rope, 6), (1, 0));
        assert_eq!(byte_to_lsp_pos(&rope, 9), (1, 3));
    }

    #[test]
    fn handles_crlf_and_utf16_columns() {
        let text = "Hello\r\nWorld\r\n😀";
        let rope = Rope::from_str(text);

        assert_eq!(lsp_pos_to_byte(&rope, 1, 0), 7);
        assert_eq!(byte_to_lsp_pos(&rope, 7), (1, 0));

        let byte_after_emoji = text.len();
        assert_eq!(byte_to_lsp_pos(&rope, byte_after_emoji), (2, 2));
    }
}
