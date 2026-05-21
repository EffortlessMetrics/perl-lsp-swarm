use crate::PerlLexer;

impl PerlLexer<'_> {
    #[inline]
    const fn is_newline_byte(byte: u8) -> bool {
        matches!(byte, b'\n' | b'\r')
    }

    #[inline]
    pub(crate) fn trailing_ws_only(bytes: &[u8], mut p: usize) -> bool {
        while let Some(&byte) = bytes.get(p) {
            if Self::is_newline_byte(byte) {
                return true;
            }

            match byte {
                b' ' | b'\t' => p += 1,
                _ => return false,
            }
        }
        true
    }

    #[inline]
    pub(crate) fn consume_newline(&mut self) {
        if self.position >= self.input.len() {
            return;
        }

        match self.input_bytes[self.position] {
            b'\r' => {
                self.position += 1;
                if self.position < self.input.len() && self.input_bytes[self.position] == b'\n' {
                    self.position += 1;
                }
            }
            b'\n' => self.advance(),
            _ => return,
        }

        self.after_newline = true;
        self.line_start_offset = self.position;
    }

    #[inline]
    pub(crate) fn find_line_end(bytes: &[u8], start: usize) -> (usize, usize) {
        if start >= bytes.len() {
            return (start, start);
        }

        let end = bytes[start..]
            .iter()
            .position(|&byte| Self::is_newline_byte(byte))
            .map_or(bytes.len(), |offset| start + offset);
        (end, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_ws_only_stops_at_newline() {
        let bytes = b"\t  \nnot checked";
        assert!(PerlLexer::trailing_ws_only(bytes, 0));
    }

    #[test]
    fn trailing_ws_only_rejects_non_whitespace_before_newline() {
        let bytes = b"\t x\n";
        assert!(!PerlLexer::trailing_ws_only(bytes, 0));
    }

    #[test]
    fn newline_helper_covers_cr_and_lf_only() {
        assert!(PerlLexer::is_newline_byte(b'\n'));
        assert!(PerlLexer::is_newline_byte(b'\r'));
        assert!(!PerlLexer::is_newline_byte(b'x'));
    }

    #[test]
    fn find_line_end_handles_crlf_and_bounds() {
        let bytes = b"first\r\nsecond";
        assert_eq!(PerlLexer::find_line_end(bytes, 0), (5, 5));
        assert_eq!(PerlLexer::find_line_end(bytes, 7), (13, 13));
        assert_eq!(PerlLexer::find_line_end(bytes, bytes.len()), (13, 13));
        assert_eq!(PerlLexer::find_line_end(bytes, bytes.len() + 1), (14, 14));
    }
}
