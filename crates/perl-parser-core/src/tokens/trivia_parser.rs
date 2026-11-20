//! Canonical parser-backed trivia surface.
//!
//! [`TriviaPreservingParser`] delegates all AST construction and recovery to
//! [`crate::Parser`]. It additionally retains the exact source and a flat trivia
//! token inventory. Per-node ownership and complete byte partitioning are not
//! inferred here; #7101 owns that generation-bound geometry contract.

use crate::{ParseOutput, Parser};
use perl_lexer::{PerlLexer, TokenType};
use perl_position_tracking::{Position, Range};

use super::trivia::{Trivia, TriviaToken};

/// Result of canonical parsing with exact source and collected trivia.
///
/// The AST and diagnostics are the canonical [`ParseOutput`]. `trivia` is a
/// source-ordered compatibility inventory, not yet a per-node attachment map.
#[derive(Debug, Clone)]
pub struct TriviaParseOutput {
    /// Canonical parser output, including AST, diagnostics, recovery, and budget state.
    pub parse: ParseOutput,
    /// Source-ordered trivia tokens collected around meaningful lexer tokens.
    pub trivia: Vec<TriviaToken>,
    source: String,
}

impl TriviaParseOutput {
    /// Return the exact source bytes supplied to the parser.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Consume the result into exact source, canonical parse output, and trivia inventory.
    #[must_use]
    pub fn into_parts(self) -> (String, ParseOutput, Vec<TriviaToken>) {
        (self.source, self.parse, self.trivia)
    }
}

/// Parser facade that preserves exact source and trivia while using the canonical parser.
pub struct TriviaPreservingParser {
    source: String,
}

impl TriviaPreservingParser {
    /// Create a canonical parser-backed trivia parse for owned source.
    #[must_use]
    pub fn new(source: String) -> Self {
        Self { source }
    }

    /// Parse through [`crate::Parser`] and retain source-ordered trivia.
    #[must_use]
    pub fn parse(self) -> TriviaParseOutput {
        let trivia = TriviaScanner::new(&self.source).collect();
        let mut parser = Parser::new(&self.source);
        let parse = parser.parse_with_recovery();

        TriviaParseOutput { parse, trivia, source: self.source }
    }
}

/// Return the exact source retained by a trivia parse.
#[must_use]
pub fn source_with_trivia(output: &TriviaParseOutput) -> &str {
    output.source()
}

/// Return exact valid source retained by a trivia parse.
///
/// This compatibility name no longer debug-renders an AST. New code should use
/// [`source_with_trivia`] to make the source-preserving contract explicit.
#[deprecated(
    note = "renamed to source_with_trivia; this returns exact source, not formatted AST debug text"
)]
#[must_use]
pub fn format_with_trivia(output: &TriviaParseOutput) -> String {
    source_with_trivia(output).to_string()
}

struct TriviaScanner<'a> {
    source: &'a str,
    position: usize,
    positions: PositionTracker<'a>,
}

impl<'a> TriviaScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, position: 0, positions: PositionTracker::new(source) }
    }

    fn collect(mut self) -> Vec<TriviaToken> {
        let mut all = Vec::new();
        let mut lexer = PerlLexer::with_body_tokens(self.source);

        while let Some(token) = lexer.next_token() {
            if self.position < self.source.len()
                && self.at_line_start(self.position)
                && self.is_pod_start(self.position)
            {
                all.extend(self.collect_trivia_at_current(self.source.len()));
            }
            if token.start > self.position {
                all.extend(self.collect_trivia_at_current(token.start));
            }
            if matches!(token.token_type, TokenType::EOF) {
                break;
            }
            self.position = self.position.max(token.end.min(self.source.len()));
        }

        if self.position < self.source.len() {
            all.extend(self.collect_trivia_at_current(self.source.len()));
        }

        all
    }

    fn collect_trivia_at_current(&mut self, limit: usize) -> Vec<TriviaToken> {
        let mut trivia = Vec::new();
        let bytes = self.source.as_bytes();
        let limit = limit.min(self.source.len());

        while self.position < limit {
            match bytes[self.position] {
                b' ' | b'\t' | b'\r' => {
                    let start = self.position;
                    while self.position < limit
                        && matches!(bytes[self.position], b' ' | b'\t' | b'\r')
                    {
                        self.position += 1;
                    }
                    trivia.push(self.token(
                        Trivia::Whitespace(self.source[start..self.position].to_string()),
                        start,
                        self.position,
                    ));
                }
                b'\n' => {
                    let start = self.position;
                    self.position += 1;
                    trivia.push(self.token(Trivia::Newline, start, self.position));
                }
                b'#' => {
                    let start = self.position;
                    while self.position < limit && bytes[self.position] != b'\n' {
                        self.position += 1;
                    }
                    trivia.push(self.token(
                        Trivia::LineComment(self.source[start..self.position].to_string()),
                        start,
                        self.position,
                    ));
                }
                b'=' if self.at_line_start(self.position) && self.is_pod_start(self.position) => {
                    let start = self.position;
                    self.position = self.find_pod_end(self.position).min(limit);
                    trivia.push(self.token(
                        Trivia::PodComment(self.source[start..self.position].to_string()),
                        start,
                        self.position,
                    ));
                }
                byte if byte >= 128 => {
                    let Some(ch) = self.source[self.position..].chars().next() else {
                        break;
                    };
                    if !ch.is_whitespace() {
                        break;
                    }
                    let start = self.position;
                    self.position += ch.len_utf8();
                    trivia.push(self.token(
                        Trivia::Whitespace(self.source[start..self.position].to_string()),
                        start,
                        self.position,
                    ));
                }
                _ => break,
            }
        }

        trivia
    }

    fn token(&self, trivia: Trivia, start: usize, end: usize) -> TriviaToken {
        TriviaToken::new(
            trivia,
            Range::new(
                self.positions.offset_to_position(start),
                self.positions.offset_to_position(end),
            ),
        )
    }

    fn at_line_start(&self, offset: usize) -> bool {
        offset == 0 || self.source.as_bytes().get(offset.saturating_sub(1)) == Some(&b'\n')
    }

    fn is_pod_start(&self, offset: usize) -> bool {
        let remaining = &self.source[offset..];
        if Self::is_cut_directive(remaining) {
            return true;
        }
        remaining
            .strip_prefix('=')
            .and_then(|rest| rest.chars().next())
            .is_some_and(|command| command.is_ascii_alphabetic())
    }

    /// True when `remaining` starts with a word-bounded `=cut` POD directive.
    ///
    /// `=cut` and `=cut;` are directives; `=cutlery` and `=cut_x` are distinct commands.
    fn is_cut_directive(remaining: &str) -> bool {
        remaining.as_bytes().starts_with(b"=cut")
            && remaining.as_bytes().get(4).is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_')
    }

    fn line_start_after(&self, offset: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut index = offset;
        while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b'\r' {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'\r' {
            index += 1;
            if index < bytes.len() && bytes[index] == b'\n' {
                index += 1;
            }
        } else if index < bytes.len() && bytes[index] == b'\n' {
            index += 1;
        }
        index
    }

    fn find_pod_end(&self, start: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut search_start = start;
        if Self::is_cut_directive(&self.source[start..]) {
            search_start = self.line_start_after(start);
        }
        let mut position = search_start;

        while position < self.source.len() {
            if self.at_line_start(position) && Self::is_cut_directive(&self.source[position..]) {
                position += 4;
                while position < self.source.len()
                    && bytes[position] != b'\n'
                    && bytes[position] != b'\r'
                {
                    position += 1;
                }
                if position < self.source.len() && bytes[position] == b'\r' {
                    position += 1;
                    if position < self.source.len() && bytes[position] == b'\n' {
                        position += 1;
                    }
                } else if position < self.source.len() && bytes[position] == b'\n' {
                    position += 1;
                }
                return position;
            }
            position += 1;
        }

        self.source.len()
    }
}

#[cfg(test)]
mod stray_cut_pod_tests {
    use super::{TriviaParseOutput, TriviaPreservingParser};
    use crate::trivia::Trivia;

    fn pod_text(output: &TriviaParseOutput) -> Option<&str> {
        output.trivia.iter().find_map(|token| {
            if let Trivia::PodComment(text) = &token.trivia { Some(text.as_str()) } else { None }
        })
    }

    #[test]
    fn stray_cut_at_statement_start_opens_pod_to_eof() {
        let source = "print 1;\n=cut\nDoc\n".to_string();
        let output = TriviaPreservingParser::new(source).parse();
        let pod = pod_text(&output).expect("stray =cut must produce PodComment trivia");
        assert_eq!(pod, "=cut\nDoc\n");
    }

    #[test]
    fn stray_cut_opens_pod_until_the_next_cut_directive() {
        let source = "print 1;\n=cut\nDoc\n=cut\nmore();\n".to_string();
        let output = TriviaPreservingParser::new(source).parse();
        let pod = pod_text(&output).expect("stray =cut must produce PodComment trivia");
        assert_eq!(pod, "=cut\nDoc\n=cut\n");
    }

    #[test]
    fn cutlery_does_not_terminate_stray_cut_pod_block() {
        let source = "print 1;\n=cut\n=cutlery\nstill pod\n=cut\n".to_string();
        let output = TriviaPreservingParser::new(source).parse();
        let pod = pod_text(&output).expect("stray =cut must produce PodComment trivia");
        assert!(
            pod.contains("=cutlery\nstill pod"),
            "cutlery must remain inside the POD block: {pod:?}"
        );
        assert!(pod.ends_with("=cut\n"), "pod must end at the real =cut: {pod:?}");
    }
}

struct PositionTracker<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> PositionTracker<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (offset, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { source, line_starts }
    }

    fn offset_to_position(&self, offset: usize) -> Position {
        let line =
            self.line_starts.binary_search(&offset).unwrap_or_else(|index| index.saturating_sub(1));
        let line_start = self.line_starts[line];
        let line_number = u32::try_from(line.saturating_add(1)).unwrap_or(u32::MAX);
        let column = self.source[line_start..offset].chars().count().saturating_add(1);
        Position::new(offset, line_number, u32::try_from(column).unwrap_or(u32::MAX))
    }
}
