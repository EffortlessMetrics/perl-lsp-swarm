//! Bare `/pattern/` regex literals.
//!
//! `/` is a regex opener only in [`LexerMode::ExpectTerm`]. Unterminated
//! literals must emit a recovery token covering the consumed bytes so
//! [`PerlLexer::next_token`] can still honor its EOF contract (#12504).

use std::sync::Arc;

use crate::PerlLexer;
use crate::limits::MAX_REGEX_PARSE_STEPS;
use crate::mode::LexerMode;
use crate::quote_handler;
use crate::token::{Token, TokenType};

use super::{empty_arc, truncate_preview};

impl PerlLexer<'_> {
    /// Consume every alphanumeric character after a regex closer.
    ///
    /// Invalid flags stay on the token so the parser can reject them (MUT_005).
    pub(crate) fn parse_regex_modifiers(&mut self, _spec: &quote_handler::ModSpec) {
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_alphanumeric() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Parse a regex literal starting with `/`.
    ///
    /// Always returns a token: closed `RegexMatch`, budget `UnknownRest`, or
    /// unterminated `Error`. Callers must not treat this as a fallible probe.
    ///
    /// Budget protection (Issue #422):
    /// - `MAX_REGEX_PARSE_STEPS` bounds literal scanning before the byte budget
    /// - `MAX_REGEX_BYTES` bounds total bytes consumed in a single regex literal
    /// - Budget exceeded → `UnknownRest` with empty text over
    ///   `[start, input.len())`: geometry-only recovery that never copies the
    ///   unbounded remainder (crate-level budget-stop recovery contract;
    ///   `tests/budget_recovery_contract.rs` pins the shape). The
    ///   line-bounded unterminated `Error` below is a different,
    ///   payload-carrying shape precisely because its span is bounded.
    pub(crate) fn parse_regex(&mut self, start: usize) -> Token {
        self.advance(); // Skip opening /

        let mut regex_parse_steps: usize = 0;
        let mut in_character_class = false;

        while let Some(ch) = self.current_char() {
            regex_parse_steps += 1;
            if regex_parse_steps > MAX_REGEX_PARSE_STEPS {
                #[cfg(debug_assertions)]
                {
                    let text = &self.input[start..self.position];
                    let preview = truncate_preview(text, 50);
                    tracing::debug!(
                        limit = MAX_REGEX_PARSE_STEPS,
                        pattern_preview = %preview,
                        "Regex parse step budget exceeded"
                    );
                }
                self.position = self.input.len();
                return Token {
                    token_type: TokenType::UnknownRest,
                    text: empty_arc(),
                    start,
                    end: self.position,
                };
            }

            if let Some(token) = self.budget_guard(start, 0) {
                return token;
            }

            match ch {
                '/' if !in_character_class => {
                    self.advance();
                    self.parse_regex_modifiers(&quote_handler::M_SPEC);

                    let text = &self.input[start..self.position];
                    self.mode = LexerMode::ExpectOperator;

                    return Token {
                        token_type: TokenType::RegexMatch,
                        text: Arc::from(text),
                        start,
                        end: self.position,
                    };
                }
                '\\' => {
                    self.advance();
                    if self.current_char().is_some() {
                        self.advance();
                    }
                }
                '[' => {
                    in_character_class = true;
                    self.advance();
                }
                ']' if in_character_class => {
                    in_character_class = false;
                    self.advance();
                }
                _ => self.advance(),
            }
        }

        self.unterminated_regex_error(start)
    }

    fn unterminated_regex_error(&mut self, start: usize) -> Token {
        // Same line-bounded recovery as unterminated strings (#5090): keep later
        // statements lexable instead of swallowing the rest of the file.
        // Inputs without a newline (including `/\0`) still cover through EOF.
        let end = self.line_bounded_unclosed_end(start);
        self.position = end;
        self.mode = LexerMode::ExpectTerm;
        Token {
            token_type: TokenType::Error(Arc::from("unterminated regex")),
            text: Arc::from(&self.input[start..end]),
            start,
            end,
        }
    }
}
