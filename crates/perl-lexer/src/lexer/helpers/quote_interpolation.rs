//! Interpolation segmentation for quote-like and heredoc bodies (#8779).
//!
//! Applies the #8244 ordinary-string interpolation policy to `qq` bodies and
//! interpolating heredoc bodies: segments are produced during the original
//! body scan (never by rescanning completed token text), non-interpolating
//! forms never consume the setting, and `qx`/backtick bodies are an explicit
//! intentional boundary that stays opaque under every configuration.
//!
//! `scan_interpolation_island` mirrors the `$`/`@` arms of
//! `parse_double_quoted_string` one-for-one — including the literal-tail
//! dispositions for non-interpolating `->method` text (#5428) — so the
//! segmentation policy is one policy. The
//! `quote_heredoc_interpolation_parity` tests enforce part-sequence
//! equivalence between the ordinary-string scanner and this mirror over a
//! corpus that covers the issue's island matrix (scalar, array, hash,
//! qualified, dereference, slice, repeated-sigil, digit, `^`-control,
//! punctuation, and `::`-qualified forms).

use crate::token::StringPart;
use crate::{
    PerlLexer,
    unicode::{is_perl_identifier_continue, is_perl_identifier_start},
};
use std::sync::Arc;

use super::interpolation_scan::is_perl_punctuation_variable;
use crate::quote_handler;

impl PerlLexer<'_> {
    /// Read a delimited body, segmenting interpolation islands into
    /// [`StringPart`]s while the scan runs (#8779).
    ///
    /// The scan loop is the same as `read_delimited_body` (escape pairs,
    /// paired-delimiter nesting, close handling); the only addition is that
    /// `$`/`@` sigils delegate to [`Self::scan_interpolation_island`] when
    /// the interpolation setting is enabled. Escape pairs stay in the
    /// literal bucket exactly as the ordinary-string scanner keeps them raw.
    pub(crate) fn read_delimited_body_with_parts(
        &mut self,
        delim: char,
    ) -> (Vec<StringPart>, bool) {
        let paired = quote_handler::paired_close(delim);
        let close = paired.unwrap_or(delim);
        let mut parts: Vec<StringPart> = Vec::new();
        let mut literal = String::new();
        let mut depth = i32::from(paired.is_some());
        let terminator = close;

        while let Some(ch) = self.current_char() {
            if ch == '\\' {
                // Escape pairs stay raw in the literal run, exactly like the
                // ordinary-string scanner (no flush: `a\$b` is one literal).
                literal.push(ch);
                self.advance();
                if let Some(next) = self.current_char() {
                    literal.push(next);
                    self.advance();
                }
                continue;
            }

            if paired.is_some() && ch == delim {
                depth += 1;
                literal.push(ch);
                self.advance();
                continue;
            }

            if ch == close {
                if paired.is_some() {
                    depth -= 1;
                    if depth == 0 {
                        self.advance();
                        Self::flush_literal(&mut literal, &mut parts);
                        return (parts, true);
                    }
                    literal.push(ch);
                    self.advance();
                } else {
                    self.advance();
                    Self::flush_literal(&mut literal, &mut parts);
                    return (parts, true);
                }
                continue;
            }

            if (ch == '$' || ch == '@') && self.config.interpolation_enabled() {
                Self::flush_literal(&mut literal, &mut parts);
                self.scan_interpolation_island(ch, Some(terminator), &mut parts, &mut literal);
                continue;
            }

            literal.push(ch);
            self.advance();
        }

        // EOF reached without finding the closing delimiter. The caller turns
        // an unclosed body into its recovery token, so the partial parts are
        // still returned honestly.
        Self::flush_literal(&mut literal, &mut parts);
        (parts, false)
    }

    /// Flush a pending literal run into `parts` (empty runs are dropped),
    /// mirroring the ordinary-string scanner's flush points.
    pub(crate) fn flush_literal(literal: &mut String, parts: &mut Vec<StringPart>) {
        if !literal.is_empty() {
            parts.push(StringPart::Literal(Arc::from(literal.as_str())));
            literal.clear();
        }
    }

    /// Consume one `$`/`@` interpolation island at the current position.
    ///
    /// `self.current_char()` must be `sigil`. The island classification
    /// mirrors `parse_double_quoted_string`'s `$`/`@` arms arm-for-arm, with
    /// `terminator` playing the role of the closing quote for balanced-segment
    /// recovery. Text that the ordinary scanner returns to its literal bucket
    /// (e.g. a non-interpolating `->method` tail) is appended to `literal`
    /// here as well.
    pub(crate) fn scan_interpolation_island(
        &mut self,
        sigil: char,
        terminator: Option<char>,
        parts: &mut Vec<StringPart>,
        literal: &mut String,
    ) {
        let part_start = self.position;
        self.advance(); // consume the sigil

        if sigil == '@' {
            self.scan_at_island_tail(part_start, terminator, parts, literal);
        } else {
            self.scan_dollar_island_tail(part_start, terminator, parts, literal);
        }
    }

    /// The `@` arm: `@{expr}`, qualified arrays, `@$ref` deref chains, `@+`/`@-`.
    fn scan_at_island_tail(
        &mut self,
        part_start: usize,
        terminator: Option<char>,
        parts: &mut Vec<StringPart>,
        literal: &mut String,
    ) {
        match self.current_char() {
            Some('{') => {
                let _ =
                    self.consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
                parts.push(StringPart::Expression(Arc::from(
                    &self.input[part_start..self.position],
                )));
            }
            Some(ch)
                if is_perl_identifier_start(ch)
                    || (ch == ':' && self.peek_char(1) == Some(':')) =>
            {
                self.consume_qualified_identifier_in_string();
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some('$') => {
                while self.current_char() == Some('$') {
                    self.advance();
                }
                self.consume_qualified_identifier_in_string();
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some('+' | '-') => {
                self.advance();
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            _ => {
                // '@' not followed by an identifier or '{' — literal text.
                literal.push('@');
            }
        }
    }

    /// The `$` arm: braces, identifiers with `->`/subscript tails, digits,
    /// `^`-controls, `$#`, `$`-deref chains, `::`-qualified, punctuation.
    #[allow(clippy::too_many_lines)]
    fn scan_dollar_island_tail(
        &mut self,
        part_start: usize,
        terminator: Option<char>,
        parts: &mut Vec<StringPart>,
        literal: &mut String,
    ) {
        match self.current_char() {
            Some('{') => {
                let _ =
                    self.consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
                parts.push(StringPart::Expression(Arc::from(
                    &self.input[part_start..self.position],
                )));
            }
            Some(ch) if is_perl_identifier_start(ch) => {
                self.scan_variable_with_tails(part_start, terminator, parts, literal);
            }
            Some(ch) if ch.is_ascii_digit() => {
                self.advance();
                while self.current_char().is_some_and(|c| c.is_ascii_digit()) {
                    self.advance();
                }
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some('^') => {
                self.advance();
                if self.current_char().is_some_and(|c| c.is_ascii_uppercase()) {
                    self.advance();
                }
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some('#') => {
                self.advance();
                if self.current_char() == Some('{') {
                    let _ = self
                        .consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
                } else {
                    while self.current_char() == Some('$') {
                        self.advance();
                    }
                    self.consume_qualified_identifier_in_string();
                }
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some('$') => {
                let mut dollar_run = 0usize;
                while self.peek_char(dollar_run) == Some('$') {
                    dollar_run += 1;
                }
                let after_run = self.peek_char(dollar_run);
                if after_run == Some('{') {
                    for _ in 0..dollar_run {
                        self.advance();
                    }
                    let _ = self
                        .consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
                    parts.push(StringPart::Expression(Arc::from(
                        &self.input[part_start..self.position],
                    )));
                } else if after_run.is_some_and(is_perl_identifier_start) {
                    for _ in 0..dollar_run {
                        self.advance();
                    }
                    self.consume_qualified_identifier_in_string();
                    parts.push(StringPart::Variable(Arc::from(
                        &self.input[part_start..self.position],
                    )));
                    if self.matches_bytes(b"->")
                        && matches!(self.peek_byte(2), Some(b'[') | Some(b'{'))
                    {
                        let tail_start = self.position;
                        self.advance();
                        self.advance();
                        if self.current_char() == Some('[') {
                            let _ = self.consume_balanced_segment_in_string_with_terminator(
                                '[', ']', terminator,
                            );
                        } else {
                            let _ = self.consume_balanced_segment_in_string_with_terminator(
                                '{', '}', terminator,
                            );
                        }
                        parts.push(StringPart::MethodCall(Arc::from(
                            &self.input[tail_start..self.position],
                        )));
                    } else if self.current_char() == Some('[') {
                        let tail_start = self.position;
                        let _ = self.consume_balanced_segment_in_string_with_terminator(
                            '[', ']', terminator,
                        );
                        parts.push(StringPart::ArraySlice(Arc::from(
                            &self.input[tail_start..self.position],
                        )));
                    } else if self.current_char() == Some('{') {
                        let tail_start = self.position;
                        let _ = self.consume_balanced_segment_in_string_with_terminator(
                            '{', '}', terminator,
                        );
                        parts.push(StringPart::Expression(Arc::from(
                            &self.input[tail_start..self.position],
                        )));
                    }
                } else if after_run.is_some_and(|c| c.is_ascii_digit()) {
                    for _ in 0..dollar_run {
                        self.advance();
                    }
                    while self.current_char().is_some_and(|c| c.is_ascii_digit()) {
                        self.advance();
                    }
                    parts.push(StringPart::Variable(Arc::from(
                        &self.input[part_start..self.position],
                    )));
                } else {
                    for _ in 0..dollar_run {
                        self.advance();
                    }
                    parts.push(StringPart::Variable(Arc::from(
                        &self.input[part_start..self.position],
                    )));
                }
            }
            Some(':') if self.peek_char(1) == Some(':') => {
                self.consume_qualified_identifier_in_string();
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            Some(ch) if is_perl_punctuation_variable(ch) => {
                self.advance();
                parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));
            }
            _ => {
                // Unrecognized '$' — literal character.
                literal.push('$');
            }
        }
    }

    /// Identifier variable plus the `->`-subscript / `[slice]` / `{subscript}`
    /// tails. A bare `->method` tail is NOT an interpolation (#5428): the
    /// arrow and name return to the literal bucket.
    fn scan_variable_with_tails(
        &mut self,
        part_start: usize,
        terminator: Option<char>,
        parts: &mut Vec<StringPart>,
        literal: &mut String,
    ) {
        let var_start = self.position;

        // Fast path for ASCII identifier continuation (mirrors the ordinary
        // scanner's byte loop).
        while self.position < self.input_bytes.len() {
            let byte = self.input_bytes[self.position];
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.position += 1;
            } else if byte >= 128 {
                if let Some(ch) = self.current_char() {
                    if is_perl_identifier_continue(ch) {
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if self.position == var_start {
            return;
        }

        parts.push(StringPart::Variable(Arc::from(&self.input[part_start..self.position])));

        if self.matches_bytes(b"->") {
            let tail_start = self.position;
            self.advance();
            self.advance();

            match self.current_char() {
                Some('[') => {
                    let _ = self
                        .consume_balanced_segment_in_string_with_terminator('[', ']', terminator);
                    parts.push(StringPart::MethodCall(Arc::from(
                        &self.input[tail_start..self.position],
                    )));
                }
                Some('{') => {
                    let _ = self
                        .consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
                    parts.push(StringPart::MethodCall(Arc::from(
                        &self.input[tail_start..self.position],
                    )));
                }
                Some('(') => {
                    let _ = self
                        .consume_balanced_segment_in_string_with_terminator('(', ')', terminator);
                    parts.push(StringPart::MethodCall(Arc::from(
                        &self.input[tail_start..self.position],
                    )));
                }
                Some(ch) if is_perl_identifier_start(ch) => {
                    // Bare method call: literal tail (#5428).
                    while self.position < self.input_bytes.len() {
                        let byte = self.input_bytes[self.position];
                        if byte.is_ascii_alphanumeric() || byte == b'_' {
                            self.position += 1;
                        } else if byte >= 128 {
                            if let Some(next) = self.current_char() {
                                if is_perl_identifier_continue(next) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let tail_text = &self.input[tail_start..self.position];
                    if !tail_text.is_empty() {
                        literal.reserve(tail_text.len());
                        literal.push_str(tail_text);
                    }
                }
                _ => {
                    // `->` followed by neither subscript nor name: literal.
                    let tail_text = &self.input[tail_start..self.position];
                    if !tail_text.is_empty() {
                        literal.reserve(tail_text.len());
                        literal.push_str(tail_text);
                    }
                }
            }
        } else if self.current_char() == Some('[') {
            let tail_start = self.position;
            let _ = self.consume_balanced_segment_in_string_with_terminator('[', ']', terminator);
            parts.push(StringPart::ArraySlice(Arc::from(&self.input[tail_start..self.position])));
        } else if self.current_char() == Some('{') {
            let tail_start = self.position;
            let _ = self.consume_balanced_segment_in_string_with_terminator('{', '}', terminator);
            parts.push(StringPart::Expression(Arc::from(&self.input[tail_start..self.position])));
        }
    }

    /// Segment an interpolating heredoc body range `[body_start, body_end)`
    /// in place (#8779): position is moved over the body, islands are
    /// classified by the same scanner as `qq`, and the position is restored.
    /// The terminator is a newline: heredoc bodies have no closing delimiter.
    /// The body range includes its final line break before the terminator line;
    /// that newline is content and remains in the last literal part.
    pub(crate) fn segment_heredoc_body(
        &mut self,
        body_start: usize,
        body_end: usize,
    ) -> Vec<StringPart> {
        let saved_position = self.position;
        let body_end = body_end.min(self.input.len());
        if !self.config.interpolation_enabled() {
            self.position = saved_position;
            return vec![StringPart::Literal(Arc::from(&self.input[body_start..body_end]))];
        }
        self.position = body_start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut literal = String::new();

        while self.position < body_end {
            let Some(ch) = self.current_char() else { break };
            if ch == '\\' {
                literal.push(ch);
                self.advance();
                if self.position < body_end
                    && let Some(next) = self.current_char()
                {
                    literal.push(next);
                    self.advance();
                }
                continue;
            }
            if (ch == '$' || ch == '@') && self.config.interpolation_enabled() {
                Self::flush_literal(&mut literal, &mut parts);
                self.scan_interpolation_island(ch, None, &mut parts, &mut literal);
                continue;
            }
            literal.push(ch);
            self.advance();
        }

        Self::flush_literal(&mut literal, &mut parts);
        self.position = saved_position;
        parts
    }
}
