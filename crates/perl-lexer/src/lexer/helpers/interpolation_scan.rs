//! Scanning helpers shared by the `$`/`@` interpolation arms of
//! `parse_double_quoted_string`.

use crate::{
    PerlLexer,
    unicode::{is_perl_identifier_continue, is_perl_identifier_start},
};

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
///
/// `:` *is* included: `$:` is Perl's format line-break set and it interpolates
/// (verified against real perl 5.38.2: `$: = "S"; print "[$:foo]"` prints
/// `[Sfoo]`, i.e. `$:` is the variable and `foo` stays literal). It is only a
/// punctuation variable when the *next* character is not a second `:` — `$::`
/// starts a package-qualified name (`"$::"` interpolates `$main::`, and
/// `"$:::foo"` prints `$main::` followed by the literal `:foo`). Callers must
/// therefore test for the `::` package form *before* consulting this set; the
/// `$` arm of `parse_double_quoted_string` orders its match arms that way.
#[inline]
pub(crate) const fn is_perl_punctuation_variable(ch: char) -> bool {
    matches!(
        ch,
        ':' | '?'
            | '!'
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
    ///
    /// A `'` is the old-style package separator, but only when it is directly
    /// followed by an identifier-start character. Verified against real perl
    /// 5.38.2: `@Foo::Bar=(1,2); print "@Foo'Bar"` prints `1 2` (with the
    /// "Old package separator used in string" deprecation warning), while
    /// `@foo=(1,2); print "@foo'"` prints `1 2'` and `print "@foo'9"` prints
    /// `1 2'9` — a `'` that does not begin a further name segment stays
    /// literal text. `is_perl_identifier_continue` accepts `'` unconditionally
    /// (it serves bare-word identifiers, where there is no closing delimiter to
    /// protect), so the `'` case is tested first here rather than falling into
    /// it.
    ///
    /// `terminator` is the active quote close (`Some('"')` for the ordinary
    /// scanner, the `qq` delimiter, or `None` for heredoc bodies, which have
    /// no close). A separator fold that would consume the terminator loses the
    /// close instead (`qq:$a::b:`, `qq'$foo'bar'`), so the terminator wins and
    /// the scan stops at the name read so far.
    #[inline]
    pub(crate) fn consume_qualified_identifier_in_string(&mut self, terminator: Option<char>) {
        while let Some(ch) = self.current_char() {
            if ch == '\'' {
                if terminator != Some(ch) && self.peek_char(1).is_some_and(is_perl_identifier_start)
                {
                    self.advance();
                } else {
                    break;
                }
            } else if is_perl_identifier_continue(ch) {
                self.advance();
            } else if ch == ':' && self.peek_char(1) == Some(':') && terminator != Some(ch) {
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
            ':', '?', '!', '@', '&', '`', '\'', '.', '/', '\\', '|', '+', '-', '[', ']', '~', '=',
            '%', ',', ';', '>', '<', ')', '(',
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
        for ch in ['a', 'Z', '_', '0', '9', '$', '{', '#', '^', '*', ' '] {
            assert!(!is_perl_punctuation_variable(ch), "{ch:?} must not be a punctuation variable");
        }
    }

    #[test]
    fn consume_qualified_identifier_in_string_folds_package_separators() {
        let mut lexer = PerlLexer::new("main::array rest");
        lexer.consume_qualified_identifier_in_string(None);

        assert_eq!(lexer.position, "main::array".len());
    }

    #[test]
    fn consume_qualified_identifier_in_string_stops_at_a_lone_colon() {
        let mut lexer = PerlLexer::new("arr:tail");
        lexer.consume_qualified_identifier_in_string(None);

        assert_eq!(lexer.position, "arr".len());
    }

    #[test]
    fn consume_qualified_identifier_in_string_folds_old_style_apostrophe_separators() {
        let mut lexer = PerlLexer::new("Foo'Bar rest");
        lexer.consume_qualified_identifier_in_string(None);

        assert_eq!(lexer.position, "Foo'Bar".len());
    }

    #[test]
    fn consume_qualified_identifier_in_string_stops_at_a_non_separating_apostrophe() {
        // perl 5.38.2: `print "@foo'"` prints `1 2'` and `print "@foo'9"`
        // prints `1 2'9` — the apostrophe is only a separator when a further
        // name segment follows it.
        for (input, expected) in [("foo'", 3), ("foo'9", 3), ("foo''bar", 3)] {
            let mut lexer = PerlLexer::new(input);
            lexer.consume_qualified_identifier_in_string(None);

            assert_eq!(lexer.position, expected, "scanning {input:?} must stop at {expected}");
        }
    }

    #[test]
    fn consume_qualified_identifier_in_string_consumes_nothing_at_a_non_identifier() {
        let mut lexer = PerlLexer::new("+rest");
        lexer.consume_qualified_identifier_in_string(None);

        assert_eq!(lexer.position, 0);
    }

    /// Call-observation over every call site in the scan loop.
    ///
    /// The tests above each drive one path and only observe the final
    /// position, so an implementation that reached the right end offset by a
    /// different route — for example advancing two bytes for a *lone* `:`, or
    /// consuming a byte before testing `is_perl_identifier_continue` — would
    /// still pass them. This observes the loop one call at a time: it runs the
    /// scan from every start offset of a single input that mixes all three
    /// branches (identifier-continue, `::` pair, and the terminating `break`)
    /// and pins the exact offset each run stops at.
    ///
    /// Concretely, for `a::b:c` the expected stop offset from each start is:
    ///
    /// | start | at  | stops at | why                                       |
    /// |-------|-----|----------|-------------------------------------------|
    /// | 0     | `a` | 4        | `a`, `::`, `b`, then the lone `:` breaks   |
    /// | 1     | `:` | 4        | `::` pair, `b`, then the lone `:` breaks   |
    /// | 2     | `:` | 2        | lone `:` (next is `b`), immediate break    |
    /// | 3     | `b` | 4        | one identifier char, then the lone `:`     |
    /// | 4     | `:` | 4        | lone `:` (next is `c`), immediate break    |
    /// | 5     | `c` | 6        | trailing identifier char to end of input   |
    /// | 6     | eof | 6        | `current_char()` is None, loop never runs  |
    #[test]
    fn consume_qualified_identifier_in_string_call_presence_observer() {
        const INPUT: &str = "a::b:c";
        const EXPECTED_STOPS: [usize; 7] = [4, 4, 2, 4, 4, 6, 6];

        for (start, expected_stop) in EXPECTED_STOPS.into_iter().enumerate() {
            let mut lexer = PerlLexer::new(INPUT);
            lexer.position = start;
            lexer.consume_qualified_identifier_in_string(None);

            assert_eq!(
                lexer.position, expected_stop,
                "scanning {INPUT:?} from offset {start} must stop at {expected_stop}"
            );
        }
    }
}
