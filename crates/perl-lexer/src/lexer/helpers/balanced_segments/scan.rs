use crate::PerlLexer;

#[derive(Copy, Clone)]
pub(super) struct SegmentDelimiters {
    pub(super) open: char,
    pub(super) close: char,
    pub(super) terminator: Option<char>,
}

#[inline]
pub(super) fn consume_balanced_segment_core(
    lexer: &mut PerlLexer<'_>,
    delimiters: SegmentDelimiters,
) -> Option<usize> {
    if lexer.current_char() != Some(delimiters.open) {
        return None;
    }

    let mut depth = 1usize;
    lexer.advance();
    while let Some(ch) = lexer.current_char() {
        match ch {
            '\\' => consume_escaped_char(lexer),
            c if is_terminator_hit(c, delimiters.terminator) => return None,
            c if c == delimiters.open => {
                depth += 1;
                lexer.advance();
            }
            c if c == delimiters.close => {
                lexer.advance();
                depth -= 1;
                if depth == 0 {
                    return Some(lexer.position);
                }
            }
            _ => lexer.advance(),
        }
    }

    None
}

#[inline]
fn consume_escaped_char(lexer: &mut PerlLexer<'_>) {
    lexer.advance();
    if lexer.current_char().is_some() {
        lexer.advance();
    }
}

#[inline]
fn is_terminator_hit(ch: char, terminator: Option<char>) -> bool {
    matches!(terminator, Some(t) if ch == t)
}
