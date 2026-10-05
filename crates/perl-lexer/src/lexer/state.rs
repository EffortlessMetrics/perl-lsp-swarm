use perl_position_tracking::Position;

use crate::config::LexerConfig;
use crate::heredoc::HeredocSpec;
use crate::mode::LexerMode;
use crate::quote_handler;
use crate::token::Token;
use perl_source_identity::{LogicalSourceId, SourceGeneration};
use std::sync::OnceLock;

/// Context-aware lexer for the Perl language.
///
/// Tokenizes Perl source text with mode tracking to correctly disambiguate
/// context-sensitive constructs such as `/` (division vs. regex) and heredocs.
pub struct PerlLexer<'a> {
    pub(crate) input: &'a str,
    pub(crate) input_bytes: &'a [u8],
    pub(crate) position: usize,
    pub(crate) mode: LexerMode,
    pub(crate) config: LexerConfig,
    pub(crate) delimiter_stack: Vec<char>,
    pub(crate) in_prototype: bool,
    pub(crate) prototype_depth: usize,
    pub(crate) after_sub: bool,
    pub(crate) after_arrow: bool,
    pub(crate) hash_brace_depth: usize,
    pub(crate) after_var_subscript: bool,
    pub(crate) paren_depth: usize,
    // Preserved for checkpoint restoration even when a tokenization path does not read it directly.
    #[allow(dead_code)]
    pub(crate) current_pos: Position,
    pub(crate) after_newline: bool,
    pub(crate) pending_heredocs: Vec<HeredocSpec>,
    pub(crate) line_start_offset: usize,
    pub(crate) emit_heredoc_body_tokens: bool,
    pub(crate) current_quote_op: Option<quote_handler::QuoteOperatorInfo>,
    pub(crate) qw_recovery_enabled: bool,
    pub(crate) eof_emitted: bool,
    /// Temporary upper bound used while segmenting a heredoc body.
    pub(crate) scan_limit: Option<usize>,
    /// Optional logical source bound by a producer (#4851 / #7747).
    pub(crate) logical_source: Option<LogicalSourceId>,
    /// Optional source generation bound by a producer (#7747).
    pub(crate) generation: SourceGeneration,
    /// Lazily computed identity for this immutable input.
    pub(crate) content_digest: OnceLock<perl_source_identity::ContentDigest>,
    /// Trivia recorded while skipping comments and POD (#17295).
    ///
    /// The main token stream never carries `Comment`/`Pod` —
    /// [`skip_whitespace_and_comments`](crate::PerlLexer::next_token) consumes
    /// them silently, and every parser-facing consumer relies on that. Spans
    /// accumulate here instead so trivia-painting surfaces (semantic tokens'
    /// advertised `comment` legend entry) can drain them after lexing. Text is
    /// the recorded source slice; large POD blocks keep an empty `text` and are
    /// sliced from the source by their `start..end` geometry, matching the
    /// `HeredocBody` convention. Operation-local accumulation: not checkpoint
    /// replay state, and cleared on restore so a resumed lexer never
    /// double-records.
    pub(crate) trivia_tokens: Vec<Token>,
}
