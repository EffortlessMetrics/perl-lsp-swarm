//! Lower-tier incremental token replay for the native parser.
//!
//! This module owns the smallest reusable incremental kernel: it preserves
//! parser tokens outside an edit, re-lexes a checkpoint-bounded window, and
//! drives the normal parser from the assembled token stream. The AST is still
//! rebuilt by the parser; callers must use the metrics and fallback reason
//! before making stronger subtree-reuse claims.

use crate::{
    ast::Node,
    engine::parser::Parser,
    error::{ParseError, ParseResult},
    token_stream::{Token, TokenStream},
};
use perl_lexer::{Checkpointable, LexerCheckpoint, PerlLexer, TokenType};
use std::fmt;
use std::ops::Range;

const CHECKPOINT_INTERVAL: usize = 256;
const MAX_INCREMENTAL_EDIT_BYTES: usize = 4096;

/// A source edit in old-source byte coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IncrementalEdit {
    /// Start byte in the old source.
    pub start_byte: usize,
    /// Exclusive end byte in the old source.
    pub old_end_byte: usize,
    /// Replacement text in the new source.
    pub new_text: String,
}

impl IncrementalEdit {
    /// Create a source edit.
    pub fn new(start_byte: usize, old_end_byte: usize, new_text: impl Into<String>) -> Self {
        Self { start_byte, old_end_byte, new_text: new_text.into() }
    }

    /// Number of bytes touched in the old source or inserted into the new source.
    pub fn touched_bytes(&self) -> usize {
        self.old_end_byte.saturating_sub(self.start_byte).saturating_add(self.new_text.len())
    }
}

/// Why an incremental attempt fell back to a complete parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackReason {
    /// The edit is too large for the bounded replay path.
    EditTooLarge,
    /// No stable checkpoint window was available.
    NoCheckpointWindow,
    /// The cached prefix or suffix could not be assembled safely.
    CacheBoundaryUnavailable,
    /// Parsing the assembled token stream failed.
    TokenReplayFailed,
}

impl fmt::Display for FallbackReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self {
            Self::EditTooLarge => "edit too large",
            Self::NoCheckpointWindow => "no checkpoint window",
            Self::CacheBoundaryUnavailable => "cache boundary unavailable",
            Self::TokenReplayFailed => "token replay failed",
        };
        formatter.write_str(description)
    }
}

/// Measurements for the most recent parse or incremental replay.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IncrementalMetrics {
    /// Whether the most recent operation parsed the complete source.
    pub full_parse: bool,
    /// Number of source bytes re-lexed for the bounded replay window.
    pub reparsed_bytes: usize,
    /// Number of cached parser tokens retained outside that window.
    pub tokens_reused: usize,
    /// Number of parser tokens produced by fresh lexing.
    pub tokens_relexed: usize,
    /// The source range re-lexed by the incremental operation.
    pub changed_range: Range<usize>,
    /// Fallback classification, when the complete-parse path was used.
    pub fallback: Option<FallbackReason>,
}

impl IncrementalMetrics {
    fn full(source_len: usize, fallback: Option<FallbackReason>) -> Self {
        Self {
            full_parse: true,
            reparsed_bytes: source_len,
            tokens_reused: 0,
            tokens_relexed: 0,
            changed_range: 0..source_len,
            fallback,
        }
    }
}

/// Cached lexical state shared by parser facades that support incremental edits.
#[derive(Debug, Clone)]
pub struct IncrementalState {
    source: String,
    tokens: Vec<Token>,
    checkpoints: Vec<LexerCheckpoint>,
    latest_metrics: IncrementalMetrics,
    diagnostics: Vec<ParseError>,
}

impl IncrementalState {
    /// Build a token and checkpoint cache for an already parsed source.
    pub fn new(source: &str) -> Self {
        let (tokens, checkpoints) = lex_full(source);
        Self {
            source: source.to_owned(),
            tokens,
            checkpoints,
            latest_metrics: IncrementalMetrics::full(source.len(), None),
            diagnostics: Vec::new(),
        }
    }

    /// Build a token cache with diagnostics from the parse that produced it.
    pub fn with_diagnostics(source: &str, diagnostics: &[ParseError]) -> Self {
        let mut state = Self::new(source);
        state.diagnostics = diagnostics.to_vec();
        state
    }

    /// Return the source represented by this cache.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Return measurements for the most recent operation.
    pub fn metrics(&self) -> &IncrementalMetrics {
        &self.latest_metrics
    }

    /// Return diagnostics from the most recent parse represented by this cache.
    pub fn diagnostics(&self) -> &[ParseError] {
        &self.diagnostics
    }

    /// Replay one edit and parse the resulting AST from the assembled tokens.
    ///
    /// If the replay window cannot be proven safe, this method preserves
    /// correctness by falling back to a complete source parse and records why.
    pub fn reparse(&mut self, new_source: &str, edit: &IncrementalEdit) -> ParseResult<Node> {
        self.validate_edit(new_source, edit)?;

        if edit.touched_bytes() > MAX_INCREMENTAL_EDIT_BYTES {
            return self.full_reparse(new_source, Some(FallbackReason::EditTooLarge));
        }

        let old_relex_start =
            self.find_before(edit.start_byte).map_or(0, |checkpoint| checkpoint.position);
        let old_relex_end = self
            .find_after(edit.old_end_byte)
            .map_or(self.source.len(), |checkpoint| checkpoint.position);

        if old_relex_end < old_relex_start || old_relex_end > self.source.len() {
            return self.full_reparse(new_source, Some(FallbackReason::NoCheckpointWindow));
        }

        let delta = edit.new_text.len() as isize
            - edit.old_end_byte.saturating_sub(edit.start_byte) as isize;
        let new_relex_end = shift_offset(old_relex_end, delta);
        let old_tokens = &self.tokens;

        let prefix = old_tokens
            .iter()
            .filter(|token| token.end <= old_relex_start)
            .cloned()
            .collect::<Vec<_>>();
        let suffix = old_tokens
            .iter()
            .filter(|token| token.start >= old_relex_end)
            .map(|token| shift_token(token, delta))
            .collect::<Option<Vec<_>>>();
        let Some(suffix) = suffix else {
            return self.full_reparse(new_source, Some(FallbackReason::CacheBoundaryUnavailable));
        };

        let left_checkpoint = self.find_before(edit.start_byte).cloned();
        let mut lexer = PerlLexer::new(new_source);
        if let Some(checkpoint) = &left_checkpoint {
            lexer.restore(checkpoint);
        }

        let mut raw_relexed = Vec::new();
        let mut replay_checkpoints = Vec::new();
        let mut next_checkpoint = old_relex_start.saturating_add(CHECKPOINT_INTERVAL);
        while let Some(token) = lexer.next_token() {
            if token.token_type == TokenType::EOF {
                break;
            }
            if token.start >= new_relex_end {
                break;
            }
            raw_relexed.push(token.clone());
            if token.end >= next_checkpoint {
                replay_checkpoints.push(lexer.checkpoint());
                next_checkpoint = token.end.saturating_add(CHECKPOINT_INTERVAL);
            }
            if token.end >= new_relex_end {
                replay_checkpoints.push(lexer.checkpoint());
                break;
            }
        }

        let reparsed = TokenStream::lexer_tokens_to_parser_tokens(raw_relexed);
        let tokens_relexed = reparsed.len();
        let mut assembled = prefix;
        assembled.extend(reparsed);
        assembled.extend(suffix);

        let mut parser = Parser::from_tokens(assembled.clone(), new_source);
        let root = match parser.parse() {
            Ok(root) => root,
            Err(_) => {
                return self.full_reparse(new_source, Some(FallbackReason::TokenReplayFailed));
            }
        };
        let diagnostics = parser.errors().to_vec();

        self.source = new_source.to_owned();
        self.tokens = assembled;
        self.checkpoints = merge_checkpoints(
            &self.checkpoints,
            replay_checkpoints,
            edit,
            old_relex_start,
            old_relex_end,
            new_source.len(),
        );
        self.latest_metrics = IncrementalMetrics {
            full_parse: false,
            reparsed_bytes: new_relex_end.saturating_sub(old_relex_start),
            tokens_reused: self.tokens.len().saturating_sub(tokens_relexed),
            tokens_relexed,
            changed_range: old_relex_start..new_relex_end,
            fallback: None,
        };
        self.diagnostics = diagnostics;
        Ok(root)
    }

    fn validate_edit(&self, new_source: &str, edit: &IncrementalEdit) -> ParseResult<()> {
        if edit.start_byte > edit.old_end_byte || edit.old_end_byte > self.source.len() {
            return Err(ParseError::syntax(
                format!(
                    "invalid incremental edit range {}..{} for source length {}",
                    edit.start_byte,
                    edit.old_end_byte,
                    self.source.len()
                ),
                edit.start_byte,
            ));
        }
        if !self.source.is_char_boundary(edit.start_byte)
            || !self.source.is_char_boundary(edit.old_end_byte)
        {
            return Err(ParseError::syntax(
                "incremental edit is not aligned to UTF-8 boundaries",
                edit.start_byte,
            ));
        }
        let expected_len = self
            .source
            .len()
            .saturating_sub(edit.old_end_byte - edit.start_byte)
            .saturating_add(edit.new_text.len());
        if new_source.len() != expected_len
            || edit.new_text.len() > new_source.len().saturating_sub(edit.start_byte)
            || new_source[edit.start_byte..edit.start_byte + edit.new_text.len()] != edit.new_text
        {
            return Err(ParseError::syntax(
                "new source does not match the incremental edit",
                edit.start_byte,
            ));
        }
        if !new_source.is_char_boundary(edit.start_byte)
            || !new_source.is_char_boundary(edit.start_byte + edit.new_text.len())
        {
            return Err(ParseError::syntax(
                "new source edit is not aligned to UTF-8 boundaries",
                edit.start_byte,
            ));
        }
        Ok(())
    }

    fn full_reparse(
        &mut self,
        source: &str,
        fallback: Option<FallbackReason>,
    ) -> ParseResult<Node> {
        let mut parser = Parser::new(source);
        let root = parser.parse()?;
        let diagnostics = parser.errors().to_vec();
        let (tokens, checkpoints) = lex_full(source);
        self.source = source.to_owned();
        self.tokens = tokens;
        self.checkpoints = checkpoints;
        let mut metrics = IncrementalMetrics::full(source.len(), fallback);
        metrics.tokens_relexed = self.tokens.len();
        self.latest_metrics = metrics;
        self.diagnostics = diagnostics;
        Ok(root)
    }

    fn find_before(&self, position: usize) -> Option<&LexerCheckpoint> {
        self.checkpoints.iter().rev().find(|checkpoint| checkpoint.position <= position)
    }

    fn find_after(&self, position: usize) -> Option<&LexerCheckpoint> {
        self.checkpoints.iter().find(|checkpoint| checkpoint.position >= position)
    }
}

fn lex_full(source: &str) -> (Vec<Token>, Vec<LexerCheckpoint>) {
    let mut lexer = PerlLexer::new(source);
    let mut checkpoints = vec![lexer.checkpoint()];
    let mut next_checkpoint = CHECKPOINT_INTERVAL;
    let mut raw_tokens = Vec::new();

    while let Some(token) = lexer.next_token() {
        if token.token_type == TokenType::EOF {
            break;
        }
        raw_tokens.push(token.clone());
        if token.end >= next_checkpoint {
            checkpoints.push(lexer.checkpoint());
            next_checkpoint = token.end.saturating_add(CHECKPOINT_INTERVAL);
        }
    }

    (TokenStream::lexer_tokens_to_parser_tokens(raw_tokens), checkpoints)
}

fn merge_checkpoints(
    old: &[LexerCheckpoint],
    replay: Vec<LexerCheckpoint>,
    edit: &IncrementalEdit,
    old_relex_start: usize,
    old_relex_end: usize,
    new_source_len: usize,
) -> Vec<LexerCheckpoint> {
    let mut checkpoints = old
        .iter()
        .filter_map(|checkpoint| {
            if checkpoint.position > old_relex_start && checkpoint.position < old_relex_end {
                return None;
            }
            let mut shifted = checkpoint.clone();
            if shifted.position >= edit.old_end_byte {
                shifted.apply_edit(
                    edit.start_byte,
                    edit.old_end_byte - edit.start_byte,
                    edit.new_text.len(),
                );
            }
            (shifted.position <= new_source_len).then_some(shifted)
        })
        .collect::<Vec<_>>();
    checkpoints.extend(replay);
    if checkpoints.is_empty() {
        checkpoints.push(LexerCheckpoint::new());
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.position);
    checkpoints.dedup_by_key(|checkpoint| checkpoint.position);
    checkpoints
}

fn shift_offset(offset: usize, delta: isize) -> usize {
    (offset as isize).saturating_add(delta).max(0) as usize
}

fn shift_token(token: &Token, delta: isize) -> Option<Token> {
    let start = shift_offset(token.start, delta);
    let end = shift_offset(token.end, delta);
    (start <= end).then(|| Token::new(token.kind, token.text.clone(), start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};

    #[test]
    fn local_edit_reuses_tokens_and_matches_fresh_parse() {
        let source = "my $value = 1;\n".repeat(40);
        let mut state = IncrementalState::new(&source);
        let start = must_some(source.find('1'));
        let new_source = source.replacen('1', "22", 1);
        let edit = IncrementalEdit::new(start, start + 1, "22");

        let incremental = must(state.reparse(&new_source, &edit));
        let mut fresh_parser = Parser::new(&new_source);
        let fresh = must(fresh_parser.parse());

        assert_eq!(incremental.to_sexp(), fresh.to_sexp());
        assert!(!state.metrics().full_parse);
        assert!(state.metrics().tokens_reused > 0);
        assert!(state.metrics().tokens_relexed > 0);
    }

    #[test]
    fn unicode_edit_is_replayed_at_character_boundaries() {
        let source = "my $name = 'old';\n".to_owned();
        let mut state = IncrementalState::new(&source);
        let start = must_some(source.find("old"));
        let new_source = source.replacen("old", "é", 1);
        let edit = IncrementalEdit::new(start, start + 3, "é");

        let incremental = must(state.reparse(&new_source, &edit));
        let mut fresh_parser = Parser::new(&new_source);
        let fresh = must(fresh_parser.parse());

        assert_eq!(incremental.to_sexp(), fresh.to_sexp());
    }

    #[test]
    fn replay_preserves_parser_diagnostics() {
        let source = "my $value = 1;\n".to_owned();
        let mut state = IncrementalState::new(&source);
        let start = must_some(source.find("1"));
        let new_source = source.replacen("1", "", 1);
        let edit = IncrementalEdit::new(start, start + 1, "");

        let _ = must(state.reparse(&new_source, &edit));

        assert!(!state.diagnostics().is_empty());
    }

    #[test]
    fn oversized_edit_records_a_full_parse_fallback() {
        let source = "my $value = 1;\n".to_owned();
        let mut state = IncrementalState::new(&source);
        let replacement = "x".repeat(MAX_INCREMENTAL_EDIT_BYTES + 1);
        let edit = IncrementalEdit::new(0, source.len(), replacement.clone());
        let new_source = replacement;

        let _ = must(state.reparse(&new_source, &edit));

        assert_eq!(state.metrics().fallback, Some(FallbackReason::EditTooLarge));
        assert!(state.metrics().full_parse);
        assert!(state.metrics().tokens_relexed > 0);
    }
}
