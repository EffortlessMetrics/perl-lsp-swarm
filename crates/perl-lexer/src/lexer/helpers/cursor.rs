use crate::PerlLexer;

impl PerlLexer<'_> {
    #[allow(clippy::inline_always)] // Performance critical in lexer hot path
    #[inline(always)]
    pub(crate) fn byte_at(bytes: &[u8], index: usize) -> u8 {
        debug_assert!(index < bytes.len());
        match bytes.get(index) {
            Some(&byte) => byte,
            None => 0,
        }
    }

    /// Ensure the internal byte offset points at a UTF-8 char boundary.
    ///
    /// This is a defensive guard against malformed intermediate offsets from
    /// complex lookahead/backtracking paths so downstream slicing never panics.
    #[inline]
    pub(crate) fn normalize_char_boundary(&mut self) {
        while self.position < self.input.len() && !self.input.is_char_boundary(self.position) {
            self.position += 1;
        }
    }

    #[allow(clippy::inline_always)] // Performance critical in lexer hot path
    #[inline(always)]
    pub(crate) fn current_char(&self) -> Option<char> {
        if self.position < self.input_bytes.len() {
            if !self.input.is_char_boundary(self.position) {
                return None;
            }
            // For ASCII, direct access is safe
            let byte = Self::byte_at(self.input_bytes, self.position);
            if byte < 128 {
                Some(byte as char)
            } else {
                // For non-ASCII, fall back to proper UTF-8 parsing
                self.input.get(self.position..).and_then(|s| s.chars().next())
            }
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn peek_char(&self, offset: usize) -> Option<char> {
        if offset > self.config.max_lookahead {
            return None;
        }
        if !self.input.is_char_boundary(self.position) {
            return None;
        }

        let pos = self.position.checked_add(offset)?;
        if pos < self.input_bytes.len() {
            // For ASCII, direct access is safe
            let byte = Self::byte_at(self.input_bytes, pos);
            if byte < 128 {
                Some(byte as char)
            } else {
                // For non-ASCII, use chars iterator
                self.input.get(self.position..).and_then(|s| s.chars().nth(offset))
            }
        } else {
            None
        }
    }

    #[allow(clippy::inline_always)] // Performance critical in lexer hot path
    #[inline(always)]
    pub(crate) fn advance(&mut self) {
        if self.position < self.input_bytes.len() {
            if !self.input.is_char_boundary(self.position) {
                self.normalize_char_boundary();
                return;
            }
            let byte = Self::byte_at(self.input_bytes, self.position);
            if byte < 128 {
                // ASCII fast path
                self.position += 1;
            } else if let Some(ch) = self.input.get(self.position..).and_then(|s| s.chars().next())
            {
                self.position += ch.len_utf8();
            }
        }
    }

    /// Fast byte-level check for ASCII characters
    #[inline]
    pub(crate) fn peek_byte(&self, offset: usize) -> Option<u8> {
        if offset > self.config.max_lookahead {
            return None;
        }

        let pos = self.position.checked_add(offset)?;
        if pos < self.input_bytes.len() { Some(self.input_bytes[pos]) } else { None }
    }

    /// Check if the next bytes match a pattern (ASCII only)
    #[inline]
    pub(crate) fn matches_bytes(&self, pattern: &[u8]) -> bool {
        let Some(end_offset) = pattern.len().checked_sub(1) else {
            return true;
        };

        if end_offset > self.config.max_lookahead {
            return false;
        }

        let Some(end) = self.position.checked_add(pattern.len()) else {
            return false;
        };

        if end <= self.input_bytes.len() {
            &self.input_bytes[self.position..end] == pattern
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LexerConfig;

    #[test]
    fn byte_at_returns_zero_for_out_of_bounds_in_release_builds()
    -> Result<(), Box<dyn std::error::Error>> {
        let bytes = b"abc";
        assert_eq!(PerlLexer::byte_at(bytes, 1), b'b');
        if !cfg!(debug_assertions) {
            assert_eq!(PerlLexer::byte_at(bytes, 9), 0);
        }
        Ok(())
    }

    #[test]
    fn current_and_peek_char_handle_ascii_and_unicode() -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("éa");

        assert_eq!(lexer.current_char(), Some('é'));
        assert_eq!(lexer.peek_char(1), Some('a'));

        lexer.advance();
        assert_eq!(lexer.position, 2);
        assert_eq!(lexer.current_char(), Some('a'));

        lexer.advance();
        assert_eq!(lexer.current_char(), None);
        Ok(())
    }

    #[test]
    fn normalize_char_boundary_advances_to_next_valid_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut lexer = PerlLexer::new("éx");
        lexer.position = 1;

        assert_eq!(lexer.current_char(), None);
        lexer.normalize_char_boundary();

        assert_eq!(lexer.position, 2);
        assert_eq!(lexer.current_char(), Some('x'));
        Ok(())
    }

    #[test]
    fn peek_helpers_respect_max_lookahead() -> Result<(), Box<dyn std::error::Error>> {
        let config =
            LexerConfig { parse_interpolation: true, track_positions: true, max_lookahead: 1 };
        let lexer = PerlLexer::with_config("abcd", config);

        assert_eq!(lexer.peek_char(1), Some('b'));
        assert_eq!(lexer.peek_char(2), None);
        assert_eq!(lexer.peek_byte(1), Some(b'b'));
        assert_eq!(lexer.peek_byte(2), None);
        assert!(lexer.matches_bytes(b"ab"));
        assert!(!lexer.matches_bytes(b"abc"));
        Ok(())
    }

    #[test]
    fn matches_bytes_handles_empty_and_overlong_patterns() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut lexer = PerlLexer::new("abc");

        assert!(lexer.matches_bytes(b""));
        assert!(lexer.matches_bytes(b"abc"));
        assert!(!lexer.matches_bytes(b"abcd"));

        lexer.position = usize::MAX;
        assert!(!lexer.matches_bytes(b"a"));
        Ok(())
    }
}
