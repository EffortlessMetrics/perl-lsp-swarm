use crate::PerlLexer;
use crate::unicode::is_perl_identifier_continue;

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

        let rest = self.input.get(self.position..)?;
        let prefix_len = offset.checked_add(1)?;
        if let Some(prefix) = rest.as_bytes().get(..prefix_len)
            && prefix.is_ascii()
        {
            return rest.as_bytes().get(offset).map(|&byte| byte as char);
        }

        rest.chars().nth(offset)
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
        if pattern.is_empty() {
            // An empty pattern matches nothing — returning true for empty
            // patterns caused incorrect delimiter matching in edge cases (#2381).
            return false;
        }

        let Some(end_offset) = pattern.len().checked_sub(1) else {
            return false;
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

    /// Read-only lookahead for the braced-variable scan (issue #3939): does
    /// the `::`-delimited chain starting AT the current position (which must
    /// be the first `:` of a `::`) consist of one or more
    /// `::identifier`-segments immediately followed by `}`, with nothing
    /// else in between?
    ///
    /// Used to decide whether a package-qualified name inside `${...}` is
    /// the ENTIRE braced content (e.g. `${Foo::bar}`, `${Foo::Bar::baz}` —
    /// fold the whole `::`-chain into one token) versus the base of a
    /// postfix/partial-deref chain (e.g. `${Foo::bar->{baz}}`,
    /// `${Foo::bar[0]}` — must NOT fold `::` here, so the qualified name
    /// stays visible to the parser's separate multi-token reconstruction
    /// path instead of being swallowed into an opaque merged Identifier).
    ///
    /// Purely a lookahead over a fresh `chars()` iterator sliced from the
    /// current byte position — never mutates `self.position`, so it is safe
    /// to call speculatively and discard the result. Unbounded (unlike
    /// `peek_char`'s `max_lookahead`-limited character lookahead), which is
    /// correct here: a real Perl package-qualified name is not pathologically
    /// long, and the loop always terminates on end-of-input, a `}`, or the
    /// first character that isn't part of a valid `::segment`.
    pub(crate) fn qualified_name_closes_brace_from_here(&self) -> bool {
        let Some(rest) = self.input.get(self.position..) else {
            return false;
        };
        let mut chars = rest.chars().peekable();
        loop {
            if chars.next() != Some(':') || chars.next() != Some(':') {
                return false;
            }
            let mut consumed_any = false;
            while chars.peek().is_some_and(|&c| is_perl_identifier_continue(c)) {
                chars.next();
                consumed_any = true;
            }
            if !consumed_any {
                return false;
            }
            // Another `::segment`, or are we at the end of the qualified name?
            let mut lookahead = chars.clone();
            if lookahead.next() == Some(':') && lookahead.next() == Some(':') {
                continue;
            }
            return chars.next() == Some('}');
        }
    }
}
#[cfg(test)]
mod tests {
    include!("../../../tests/fixtures/ripr_seam_proof_peek_char_unit.inc");

    use crate::PerlLexer;

    #[test]
    fn matches_bytes_empty_pattern_returns_false() {
        // An empty pattern should not match anything (#2381).
        let lexer = PerlLexer::new("hello");
        assert!(!lexer.matches_bytes(b""), "empty pattern must not match");
    }

    #[test]
    fn matches_bytes_non_empty_pattern_works() {
        let lexer = PerlLexer::new("hello");
        assert!(lexer.matches_bytes(b"hel"));
        assert!(!lexer.matches_bytes(b"xyz"));
    }
}
