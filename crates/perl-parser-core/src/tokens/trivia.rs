//! Trivia (comments and whitespace) tokens for parser workflows.
//!
//! This module owns trivia values and the legacy trivia lexer only. The sole
//! public parser-backed surface is [`crate::tokens::trivia_parser::TriviaPreservingParser`],
//! which delegates AST construction and recovery to the canonical [`crate::Parser`].

use perl_ast_v2::Node;
use perl_lexer::TokenType;
use perl_position_tracking::Range;

/// Trivia represents non-semantic tokens like comments and whitespace.
#[derive(Debug, Clone, PartialEq)]
pub enum Trivia {
    /// Whitespace (spaces, tabs, etc.).
    Whitespace(String),
    /// Single-line comment starting with `#`.
    LineComment(String),
    /// POD documentation.
    PodComment(String),
    /// Newline character(s).
    Newline,
}

impl Trivia {
    /// Convert trivia to its exact source text where represented.
    pub fn as_str(&self) -> &str {
        match self {
            Trivia::Whitespace(s) | Trivia::LineComment(s) | Trivia::PodComment(s) => s,
            Trivia::Newline => "\n",
        }
    }

    /// Get the stable display name for this trivia type.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Trivia::Whitespace(_) => "whitespace",
            Trivia::LineComment(_) => "comment",
            Trivia::PodComment(_) => "pod",
            Trivia::Newline => "newline",
        }
    }
}

/// Legacy AST-v2 node/trivia container.
///
/// This type is retained temporarily for source compatibility. It is not
/// produced by the canonical parser-backed trivia surface and must not be used
/// as evidence that trivia is attached to canonical AST nodes. #7101 owns the
/// generation-bound source-geometry replacement.
#[deprecated(
    note = "legacy AST-v2 container; use trivia_parser::TriviaParseOutput and await #7101 for node attachment"
)]
#[derive(Debug, Clone)]
pub struct NodeWithTrivia {
    /// The legacy AST-v2 node.
    pub node: Node,
    /// Trivia that appears before this node.
    pub leading_trivia: Vec<TriviaToken>,
    /// Trivia that appears after this node.
    pub trailing_trivia: Vec<TriviaToken>,
}

/// A trivia token with position information.
#[derive(Debug, Clone)]
pub struct TriviaToken {
    /// The trivia content.
    pub trivia: Trivia,
    /// The source range of this trivia.
    pub range: Range,
}

impl TriviaToken {
    /// Create a new trivia token with the given content and range.
    pub fn new(trivia: Trivia, range: Range) -> Self {
        Self { trivia, range }
    }
}

/// Extension trait for collecting trivia.
///
/// Implement this trait to collect leading and trailing trivia during lexing.
pub trait TriviaCollector {
    /// Collect trivia tokens before the next meaningful token.
    fn collect_leading_trivia(&mut self) -> Vec<TriviaToken>;

    /// Collect trivia tokens after a node, typically until newline.
    fn collect_trailing_trivia(&mut self) -> Vec<TriviaToken>;
}

/// Legacy lexer wrapper that preserves trivia.
///
/// New parser consumers should use
/// [`crate::tokens::trivia_parser::TriviaPreservingParser`]. This lexer remains
/// available for low-level compatibility tests and borrows its source so its
/// underlying lexer cannot outlive the caller-owned buffer.
pub struct TriviaLexer<'a> {
    /// The underlying Perl lexer.
    lexer: perl_lexer::PerlLexer<'a>,
    /// Borrowed source code.
    source: &'a str,
    /// Current position for trivia tracking.
    position: usize,
    /// Buffered trivia tokens.
    _trivia_buffer: Vec<TriviaToken>,
}

impl<'a> TriviaLexer<'a> {
    /// Create a new trivia-preserving lexer.
    pub fn new(source: &'a str) -> Self {
        Self {
            lexer: perl_lexer::PerlLexer::new(source),
            source,
            position: 0,
            _trivia_buffer: Vec::new(),
        }
    }

    /// Get the next token together with whitespace/comments that precede it.
    pub fn next_token_with_trivia(&mut self) -> Option<(perl_lexer::Token, Vec<TriviaToken>)> {
        let trivia = self.collect_trivia();
        let token = self.lexer.next_token()?;
        self.position = self.position.max(token.end);

        if matches!(token.token_type, TokenType::EOF) {
            return (!trivia.is_empty()).then_some((token, trivia));
        }

        Some((token, trivia))
    }

    fn collect_trivia(&mut self) -> Vec<TriviaToken> {
        let mut trivia = Vec::new();

        while self.position < self.source.len() {
            let remaining = &self.source[self.position..];

            if let Some(ws_len) = self.whitespace_length(remaining) {
                let ws = &remaining[..ws_len];
                let start = self.position;
                let end = start + ws_len;
                let value = if ws.chars().all(|c| c == '\n' || c == '\r') {
                    Trivia::Newline
                } else {
                    Trivia::Whitespace(ws.to_string())
                };
                trivia.push(TriviaToken::new(
                    value,
                    Range::new(
                        perl_position_tracking::Position::new(start, 0, 0),
                        perl_position_tracking::Position::new(end, 0, 0),
                    ),
                ));
                self.position += ws_len;
                continue;
            }

            if remaining.starts_with('#') {
                let comment_end = remaining.find('\n').unwrap_or(remaining.len());
                let start = self.position;
                let end = start + comment_end;
                trivia.push(TriviaToken::new(
                    Trivia::LineComment(remaining[..comment_end].to_string()),
                    Range::new(
                        perl_position_tracking::Position::new(start, 0, 0),
                        perl_position_tracking::Position::new(end, 0, 0),
                    ),
                ));
                self.position += comment_end;
                continue;
            }

            if remaining.starts_with('=')
                && (self.position == 0 || self.source.as_bytes()[self.position - 1] == b'\n')
                && let Some(pod_end) = self.find_pod_end(remaining)
            {
                let start = self.position;
                let end = start + pod_end;
                trivia.push(TriviaToken::new(
                    Trivia::PodComment(remaining[..pod_end].to_string()),
                    Range::new(
                        perl_position_tracking::Position::new(start, 0, 0),
                        perl_position_tracking::Position::new(end, 0, 0),
                    ),
                ));
                self.position += pod_end;
                continue;
            }

            break;
        }

        trivia
    }

    fn whitespace_length(&self, source: &str) -> Option<usize> {
        let mut len = 0;
        for ch in source.chars() {
            if ch.is_whitespace() && ch != '\n' && ch != '\r' {
                len += ch.len_utf8();
            } else if ch == '\n' || ch == '\r' {
                len += ch.len_utf8();
                if ch == '\r' && source[len..].starts_with('\n') {
                    len += 1;
                }
                break;
            } else {
                break;
            }
        }

        (len > 0).then_some(len)
    }

    fn find_pod_end(&self, source: &str) -> Option<usize> {
        let mut position = 0;
        for line in source.lines() {
            if line.trim() == "=cut" {
                return Some(position + line.len());
            }
            position += line.len() + 1;
        }
        Some(source.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_some;

    #[test]
    fn collects_whitespace_and_comment_trivia() {
        let source = "  # comment\n  my $x = 42;".to_string();
        let mut lexer = TriviaLexer::new(&source);

        let (_token, trivia) = must_some(lexer.next_token_with_trivia());

        assert!(trivia.len() >= 2);
        assert!(trivia.iter().any(|t| matches!(&t.trivia, Trivia::Whitespace(_))));
        assert!(trivia.iter().any(|t| matches!(&t.trivia, Trivia::LineComment(_))));
    }

    #[test]
    fn preserves_pod_trivia() {
        let source = "=head1 NAME\n\nTest\n\n=cut\n\nmy $x;".to_string();
        let mut lexer = TriviaLexer::new(&source);

        let (_, trivia) = must_some(lexer.next_token_with_trivia());

        assert!(trivia.iter().any(|t| matches!(&t.trivia, Trivia::PodComment(_))));
    }

    #[test]
    fn borrows_the_original_source_buffer() {
        let source = String::from("my $x = 42;");
        let lexer = TriviaLexer::new(&source);

        assert!(std::ptr::eq(lexer.source.as_ptr(), source.as_ptr()));
    }
}
