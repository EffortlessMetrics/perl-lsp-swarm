//! Scanning helpers shared by the `$`/`@` interpolation arms of
//! `parse_double_quoted_string`.

use crate::{PerlLexer, unicode::is_perl_identifier_continue};

/// Is `ch` one of Perl's punctuation special variables when it directly
/// follows a `$` sigil inside a double-quoted string?
///
/// Every character in this set is a real Perl punctuation variable (`$!`,
/// `$@`, `$&`, `` $` ``, `$'`, `$/`, `$,`, `$;`, `$(`, `$)`, …) and therefore
/// interpolates rather than staying literal text.
///
/// `"` is deliberately excluded. Perl does define `$"` (the list separator),
/// but inside a double-quoted string the closing delimiter wins:
/// `perl -e 'print "$"'` prints a literal `$` rather than interpolating `$"`.
/// Accepting `"` here would consume the terminating quote, turning the valid
/// string `"$"` into an unterminated-string error and mis-lexing everything
/// after it.
///
/// `\` *is* included: `$\` is Perl's output record separator and it does
/// interpolate. Verified against real perl 5.38.2 — with `$\ = "!"`,
/// `print "x$\ny"` writes `x!ny`, so the `$` sigil claims the backslash and
/// the following `n` stays literal text rather than forming a `\n` escape.
#[inline]
pub(crate) const fn is_perl_punctuation_variable(ch: char) -> bool {
    matches!(
        ch,
        '?' | '!'
            | '@'
            | '&'
            | '`'
            | '\''
            | '.'
            | '/'
            | '\\'
            | '|'
            | '+'
            | '-'
            | '['
            | ']'
            | '~'
            | '='
            | '%'
            | ','
            | ';'
            | '>'
            | '<'
            | ')'
            | '('
    )
}

impl PerlLexer<'_> {
    /// Consume an identifier that may carry `::`-qualified package segments,
    /// starting at the current position.
    ///
    /// A lone `:` is not a package separator and ends the scan; only a `::`
    /// pair is folded into the name. Verified against real perl 5.38.2:
    /// `our @array=(1,2,3); print "$#main::array"` prints `2`, so `main::array`
    /// is one name.
    #[inline]
    pub(crate) fn consume_qualified_identifier_in_string(&mut self) {
        while let Some(ch) = self.current_char() {
            if is_perl_identifier_continue(ch) {
                self.advance();
            } else if ch == ':' && self.peek_char(1) == Some(':') {
                self.advance();
                self.advance();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_perl_punctuation_variable_accepts_every_documented_special_variable() {
        for ch in [
            '?', '!', '@', '&', '`', '\'', '.', '/', '\\', '|', '+', '-', '[', ']', '~', '=', '%',
            ',', ';', '>', '<', ')', '(',
        ] {
            assert!(is_perl_punctuation_variable(ch), "{ch:?} must be a punctuation variable");
        }
    }

    #[test]
    fn is_perl_punctuation_variable_rejects_the_string_delimiter() {
        // Accepting '"' would swallow the closing quote of the enclosing string.
        assert!(!is_perl_punctuation_variable('"'));
    }

    #[test]
    fn is_perl_punctuation_variable_rejects_identifier_and_sigil_characters() {
        for ch in ['a', 'Z', '_', '0', '9', '$', '{', '#', '^', ':', '*', ' '] {
            assert!(!is_perl_punctuation_variable(ch), "{ch:?} must not be a punctuation variable");
        }
    }

    #[test]
    fn consume_qualified_identifier_in_string_folds_package_separators() {
        let mut lexer = PerlLexer::new("main::array rest");
        lexer.consume_qualified_identifier_in_string();

        assert_eq!(lexer.position, "main::array".len());
    }

    #[test]
    fn consume_qualified_identifier_in_string_stops_at_a_lone_colon() {
        let mut lexer = PerlLexer::new("arr:tail");
        lexer.consume_qualified_identifier_in_string();

        assert_eq!(lexer.position, "arr".len());
    }

    #[test]
    fn consume_qualified_identifier_in_string_consumes_nothing_at_a_non_identifier() {
        let mut lexer = PerlLexer::new("+rest");
        lexer.consume_qualified_identifier_in_string();

        assert_eq!(lexer.position, 0);
    }
}
