use perl_position_tracking::Position;

use crate::config::LexerConfig;
use crate::heredoc::HeredocSpec;
use crate::mode::LexerMode;
use crate::quote_handler;
use perl_source_identity::{LogicalSourceId, SourceGeneration};
use std::sync::OnceLock;

/// One line-initial `=cut` that opened a POD block because no POD was open
/// (`cut_opens_pod` in the whitespace/comment skipper).
///
/// The skipper consumes the block silently — no `Pod` token exists on the
/// canonical path — so this record is the only durable witness of the
/// amputation decision. Consumers (issue #18771) use it to warn that the
/// skipped text is not compiled or indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrayCutOpen {
    /// Byte offset of the line-start `=` that opened the block.
    pub cut_offset: usize,
    /// `true` when no later `=cut` closed the block, so everything from the
    /// `=cut` line to EOF was skipped.
    pub runs_to_eof: bool,
}

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
    /// Set when the current post-`sub` window has shown signature evidence
    /// (a sigil-named parameter or a default assignment), so quote-like
    /// words in it are expression operators, not prototype payload.
    pub(crate) prototype_signature_seen: bool,
    pub(crate) after_sub: bool,
    pub(crate) after_arrow: bool,
    pub(crate) hash_brace_depth: usize,
    pub(crate) after_var_subscript: bool,
    pub(crate) paren_depth: usize,
    /// Depth of currently-open parens that were opened by `print` invoked as a
    /// list operator with parens (i.e., `print(...)` at term position, with no
    /// preceding `->` or `&`). Inside those parens, in `ExpectOperator` mode,
    /// `<<` is recognized as a heredoc opener; the same `<<` outside those
    /// parens is the left-shift operator. Tracked for #16163.
    /// Open `print(...)` list-operator parens as `(enclosing_paren_depth,
    /// seen_top_level_comma)` pairs, ordered innermost-last (#16163
    /// review). The enclosing depth is the `paren_depth` at which the
    /// print opener's `(` was consumed, so a nested `)` pops only the
    /// entry it actually closes.
    pub(crate) print_list_parens: Vec<(usize, bool, bool)>,
    /// Set by the identifier-or-keyword handler when a `print` keyword/identifier
    /// at term position is immediately followed (after horizontal whitespace) by
    /// `(`. Consumed by the `(` handler in `try_delimiter` to record that the
    /// open paren belongs to a `print(...)` list-operator call.
    pub(crate) pending_print_list_paren: bool,
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
    /// Memo of the next line-start `=cut`: `(scanned_from, position)`; `None` position means none.
    pub(crate) pod_cut_cache: Option<(usize, Option<usize>)>,
    /// Every stray `=cut` (one that opened POD with no POD already open), in
    /// source order. Accumulates as the lexer passes each directive; a caller
    /// that drives the lexer to EOF sees the complete record (#18771).
    pub(crate) stray_cut_opens: Vec<StrayCutOpen>,
    /// Temporary upper bound used while segmenting a heredoc body.
    pub(crate) scan_limit: Option<usize>,
    /// Optional logical source bound by a producer (#4851 / #7747).
    pub(crate) logical_source: Option<LogicalSourceId>,
    /// Optional source generation bound by a producer (#7747).
    pub(crate) generation: SourceGeneration,
    /// Lazily computed identity for this immutable input.
    pub(crate) content_digest: OnceLock<perl_source_identity::ContentDigest>,
}
