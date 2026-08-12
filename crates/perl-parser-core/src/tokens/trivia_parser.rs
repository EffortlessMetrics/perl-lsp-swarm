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
#[deprecated(note = "renamed to source_with_trivia; this returns exact source, not formatted AST debug text")]
#[must_use]
pub fn format_with_trivia(output: &TriviaParseOutput) -> String {
    source_with_trivia(output).to_string()
}

struct TriviaScanner<'a> {
    source: &'a str,
    position: usize,
    positions: PositionTracker,
}

impl<'a> TriviaScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, position: 0, positions: PositionTracker::new(source) }
    }

    fn collect(mut self) -> Vec<TriviaToken> {
        let mut all = Vec::new();

        while self.position < self.source.len() {
            all.extend(self.collect_trivia_at_current());
            if self.position >= self.source.len() {
                break;
            }

            let token_source = &self.source[self.position..];
            let mut lexer = PerlLexer::new(token_source);
            let Some(token) = lexer.next_token() else {
                break;
            };
            if matches!(token.token_type, TokenType::EOF) {
                break;
            }

            let next = self.position.saturating_add(token.end);
            if next <= self.position {
                let Some(ch) = self.source[self.position..].chars().next() else {
                    break;
                };
                self.position = self.position.saturating_add(ch.len_utf8());
            } else {
                self.position = next.min(self.source.len());
            }
        }

        if self.position < self.source.len() {
            all.extend(self.collect_trivia_at_current());
        }

        all
    }

    fn collect_trivia_at_current(&mut self) -> Vec<TriviaToken> {
        let mut trivia = Vec::new();
        let bytes = self.source.as_bytes();

        while self.position < self.source.len() {
            match bytes[self.position] {
                b' ' | b'\t' | b'\r' => {
                    let start = self.position;
                    while self.position < self.source.len()
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
                    while self.position < self.source.len() && bytes[self.position] != b'\n' {
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
                    self.position = self.find_pod_end(self.position);
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
        [
            "=pod",
            "=head",
            "=over",
            "=item",
            "=back",
            "=begin",
            "=end",
            "=for",
            "=encoding",
        ]
        .iter()
        .any(|prefix| remaining.starts_with(prefix))
    }

    fn find_pod_end(&self, start: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut position = start;

        while position < self.source.len() {
            if self.at_line_start(position) && self.source[position..].starts_with("=cut") {
                position += 4;
                while position < self.source.len() && bytes[position] != b'\n' {
                    position += 1;
                }
                if position < self.source.len() {
                    position += 1;
                }
                return position;
            }
            position += 1;
        }

        self.source.len()
    }
}

struct PositionTracker {
    line_starts: Vec<usize>,
}

impl PositionTracker {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { line_starts }
    }

    fn offset_to_position(&self, offset: usize) -> Position {
        let line = self
            .line_starts
            .binary_search(&offset)
            .unwrap_or_else(|index| index.saturating_sub(1));
        let line_start = self.line_starts[line];
        Position::new(offset, (line + 1) as u32, (offset - line_start + 1) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    #[test]
    fn parse_uses_canonical_ast_and_collects_trivia() {
        let source = "#!/usr/bin/perl\n# comment\nmy $x = 42;\n".to_string();
        let output = TriviaPreservingParser::new(source.clone()).parse();

        assert!(matches!(output.parse.ast.kind, NodeKind::Program { .. }));
        assert!(output.parse.ast.to_sexp().contains("variable_declaration"));
        assert!(output
            .trivia
            .iter()
            .any(|token| matches!(&token.trivia, Trivia::LineComment(text) if text.starts_with("#!"))));
        assert_eq!(output.source(), source);
    }

    #[test]
    fn source_projection_is_exact_perl_not_debug_text() {
        let source = "my $x = 42; # keep\n".to_string();
        let output = TriviaPreservingParser::new(source.clone()).parse();

        assert_eq!(source_with_trivia(&output), source);
        #[allow(deprecated)]
        {
            assert_eq!(format_with_trivia(&output), source);
        }
    }

    #[test]
    fn canonical_recovery_is_preserved() {
        let source = "if (".to_string();
        let output = TriviaPreservingParser::new(source.clone()).parse();
        let mut canonical = Parser::new(&source);
        let expected = canonical.parse_with_recovery();

        assert_eq!(output.parse.ast.to_sexp(), expected.ast.to_sexp());
        assert_eq!(output.parse.diagnostics, expected.diagnostics);
        assert_eq!(output.parse.terminated_early, expected.terminated_early);
    }
}
