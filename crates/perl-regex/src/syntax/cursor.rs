pub(crate) struct RegexCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> RegexCursor<'a> {
    pub(crate) fn new(pattern: &'a str) -> Self {
        Self { bytes: pattern.as_bytes(), pos: 0 }
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn current(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    pub(crate) fn peek(&self, offset: usize) -> Option<u8> {
        self.pos.checked_add(offset).and_then(|index| self.bytes.get(index)).copied()
    }

    pub(crate) fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1).min(self.bytes.len());
    }

    pub(crate) fn advance_to(&mut self, position: usize) {
        self.pos = position.min(self.bytes.len());
    }

    pub(crate) fn skip_escape(&mut self) -> bool {
        if self.current() != Some(b'\\') {
            return false;
        }
        self.pos = self.pos.saturating_add(2).min(self.bytes.len());
        true
    }

    pub(crate) fn skip_char_class(&mut self) -> bool {
        if self.current() != Some(b'[') {
            return false;
        }
        self.bump();
        while let Some(ch) = self.current() {
            if ch == b'\\' {
                self.pos = self.pos.saturating_add(2).min(self.bytes.len());
            } else {
                self.bump();
                if ch == b']' {
                    break;
                }
            }
        }
        true
    }

    pub(crate) fn skip_comment(&mut self) -> bool {
        if self.current() != Some(b'(') || self.peek(1) != Some(b'?') || self.peek(2) != Some(b'#')
        {
            return false;
        }
        self.pos = self.pos.saturating_add(3).min(self.bytes.len());
        while let Some(ch) = self.current() {
            self.bump();
            if ch == b')' {
                break;
            }
        }
        true
    }

    pub(crate) fn skip_quoted_literal(&mut self) -> bool {
        let Some(end) = quoted_literal_end(self.bytes, self.pos) else {
            return false;
        };
        self.pos = end;
        true
    }
}

pub(crate) fn quoted_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'\\') || bytes.get(start + 1) != Some(&b'Q') {
        return None;
    }

    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' && bytes[i + 1] == b'E' {
            return Some(i + 2);
        }
        i += 1;
    }
    Some(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_escape_advances_two_bytes() {
        let mut cursor = RegexCursor::new(r"\Q");
        assert!(cursor.skip_escape());
        assert_eq!(cursor.position(), 2);
        assert!(!RegexCursor::new("a").skip_escape());
    }

    #[test]
    fn skip_char_class_handles_escaped_brackets() {
        let mut cursor = RegexCursor::new(r"[a\]b]x");
        assert!(cursor.skip_char_class());
        assert_eq!(cursor.position(), 6);
        assert_eq!(cursor.current(), Some(b'x'));
    }

    #[test]
    fn skip_comment_stops_at_first_closing_paren() {
        let mut cursor = RegexCursor::new(r"(?#comment\)more)rest");
        assert!(cursor.skip_comment());
        // Literal comment ends at the first `)`; backslash does not escape it.
        assert_eq!(cursor.position(), 12);
        assert_eq!(cursor.current(), Some(b'm'));
        assert_eq!(&r"(?#comment\)more)rest"[cursor.position()..], "more)rest");
    }

    #[test]
    fn skip_quoted_literal_consumes_q_e_span() {
        let mut cursor = RegexCursor::new(r"\Qa+)+\Ex");
        assert!(cursor.skip_quoted_literal());
        assert_eq!(cursor.position(), 8);
        assert_eq!(cursor.current(), Some(b'x'));
        assert_eq!(quoted_literal_end(br"\Qopen", 0), Some(6));
    }

    #[test]
    fn quoted_literal_end_boundaries_are_exact() {
        assert_eq!(quoted_literal_end(b"", 0), None);
        assert_eq!(quoted_literal_end(br"\", 0), None);
        assert_eq!(quoted_literal_end(br"\Q", 0), Some(2));
        assert_eq!(quoted_literal_end(br"\QE", 0), Some(3));
        assert_eq!(quoted_literal_end(br"\Q\E", 0), Some(4));
        assert_eq!(quoted_literal_end(br"x\Qy\Ez", 1), Some(6));
        assert_eq!(quoted_literal_end(br"x\Qy\Ez", 0), None);
        assert!(!RegexCursor::new("Q").skip_quoted_literal());
    }
}
