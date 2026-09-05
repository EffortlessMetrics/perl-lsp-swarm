//! Token stream adapter between `perl-lexer` output and the parser.
//!
//! Provides buffered lookahead, skips trivia tokens, and resets lexer mode at
//! statement boundaries. This stream is optimized for parser consumption rather
//! than full-fidelity token preservation.
//!
//! # Basic usage
//!
//! ```
//! use perl_parser_core::tokens::token_stream::{TokenKind, TokenStream};
//!
//! let mut stream = TokenStream::new("my $x = 42;");
//! assert!(matches!(stream.peek(), Ok(token) if token.kind() == TokenKind::My));
//!
//! while let Ok(token) = stream.next() {
//!     if token.kind() == TokenKind::Eof {
//!         break;
//!     }
//! }
//! ```
//!
//! # Pre-lexed token stream
//!
//! For incremental parsing, use [`TokenStream::from_vec`] to create a stream
//! from pre-lexed tokens without re-lexing from source:
//!
//! ```
//! use perl_parser_core::tokens::token_stream::{Token, TokenKind, TokenStream};
//!
//! let tokens = vec![
//!     Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
//!     Token::new_checked(TokenKind::ScalarSigil, "$", 3, 4).expect("valid token"),
//!     Token::new_checked(TokenKind::Identifier, "x", 4, 5).expect("valid token"),
//!     Token::new_checked(TokenKind::Assign, "=", 6, 7).expect("valid token"),
//!     Token::new_checked(TokenKind::Number, "1", 8, 9).expect("valid token"),
//!     Token::new_checked(TokenKind::Semicolon, ";", 9, 10).expect("valid token"),
//!     Token::new_checked(TokenKind::Eof, "", 10, 10).expect("valid token"),
//! ];
//! let mut stream = TokenStream::from_vec(tokens);
//! assert!(matches!(stream.peek(), Ok(t) if t.kind() == TokenKind::My));
//! ```

use crate::syntax::error::{ParseError, ParseResult};
use perl_lexer::{
    Checkpointable, LexerCheckpoint, LexerMode, PerlLexer, Token as LexerToken,
    TokenType as LexerTokenType,
};
use perl_token::TokenSpanError;
pub use perl_token::{Token, TokenKind};
use std::collections::VecDeque;

/// A parser-directed contextual token operation (issue #8128).
///
/// Contextual operations can change how not-yet-produced tokens are classified
/// after the stream has already buffered or lexed a lookahead window. Every
/// operation returns a [`ContextualOpResult`] so the caller can observe whether
/// the operation actually occurred; no contextual request is ever silently
/// reduced to lookahead-cache clearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextualTokenOp {
    /// Reset lexer state to the start of a new statement (`ExpectTerm`) and
    /// drop the affected lookahead window.
    StatementBoundaryReset,
    /// Re-derive the head lookahead token from a real captured boundary in the
    /// requested lexer context (for example `ExpectTerm` so a `/` classified as
    /// division becomes a regex delimiter).
    ReclassifyFromBoundary {
        /// Lexer context the reclassified token must be produced in.
        expected_context: LexerMode,
    },
    /// Enter format-body lexing so the next token is consumed as a format body.
    EnterFormatBody,
    /// Drop the lookahead cache without any classification change. This is
    /// stream-local housekeeping and is applicable on every backing mode.
    InvalidateLookahead,
}

impl ContextualTokenOp {
    /// Stable label used in diagnostics that report unapplied operations.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::StatementBoundaryReset => "statement_boundary_reset",
            Self::ReclassifyFromBoundary { .. } => "reclassify_from_boundary",
            Self::EnterFormatBody => "enter_format_body",
            Self::InvalidateLookahead => "invalidate_lookahead",
        }
    }
}

/// Why a buffered stream refused a contextual operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextualFallbackReason {
    /// The buffered stream carries no source identity, so no replay authority
    /// of any kind exists. Source-less `from_vec` streams are only valid for
    /// callers that never request contextual reclassification.
    NoBufferedSource,
    /// The buffered stream retains source identity but no complete checkpoint
    /// was captured at the requested boundary, so exact replay cannot be
    /// guaranteed. The incremental layer must rebuild through a live lexer.
    NoCheckpointAuthority,
}

/// Outcome of a [`ContextualTokenOp`] requested on a [`TokenStream`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextualOpResult {
    /// Applied through the live-lexer backing: real lexer state changed and the
    /// affected lookahead entries were re-derived.
    AppliedLive,
    /// Applied on the buffered backing. Today this is limited to
    /// classification-preserving lookahead invalidation; buffered
    /// classification operations refuse instead of claiming replay.
    AppliedReplay,
    /// The requested context already holds (for example, no lookahead token is
    /// in flight to reclassify); no classification change was needed.
    NotRequired,
    /// The stream cannot honor the request. The caller must rebuild through a
    /// live lexer to obtain the corrected classification; the stream state is
    /// left untouched so the refusal is observable.
    FallbackRequired {
        /// Why the request was refused.
        reason: ContextualFallbackReason,
    },
    /// The operation is not expressible as requested (for example,
    /// reclassification into a body-consumption context), independent of the
    /// backing mode. No state changed.
    Unsupported,
}

/// Which backing a [`TokenStream`] consumes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStreamBacking {
    /// A live lexer producing tokens on demand from source text.
    Live,
    /// Pre-lexed tokens; `source_retained` reports whether the stream also
    /// retains the exact source text for potential replay authority.
    Buffered {
        /// Whether exact source identity is retained alongside the buffer.
        source_retained: bool,
    },
}

/// Backing source for the token stream — either a live lexer or pre-lexed tokens.
enum TokenStreamInner<'a> {
    /// Live lexer producing tokens on demand from source text.
    ///
    /// Boxed because `PerlLexer` is substantially larger than the `Buffered`
    /// variant; without indirection the enum's size is dominated by this one
    /// arm (clippy::large_enum_variant).
    Lexer(Box<PerlLexer<'a>>),
    /// Pre-lexed token buffer; used by [`TokenStream::from_vec`] and
    /// [`TokenStream::from_vec_with_source`].
    Buffered(BufferedTokens<'a>),
}

/// Pre-lexed token buffer plus the identity needed to reason about replay.
struct BufferedTokens<'a> {
    tokens: VecDeque<Token>,
    /// Exact source identity retained by the caller, when available. Presence
    /// distinguishes [`ContextualFallbackReason::NoBufferedSource`] from
    /// [`ContextualFallbackReason::NoCheckpointAuthority`].
    source: Option<&'a str>,
}

/// Token stream that wraps perl-lexer or a pre-lexed token buffer.
///
/// Provides three-token lookahead, transparent trivia skipping (in lexer mode),
/// and parser-directed contextual token operations used by the recursive-descent
/// parser (issue #8128).
///
/// # Backing modes and contextual operations
///
/// The live backing applies contextual operations by restoring a real captured
/// boundary checkpoint, so all prefix-established lexer state (quote operators,
/// heredoc queues, delimiter stacks, statement modes) is preserved. The buffered
/// backing either reproduces the live classification through exact replay or
/// returns [`ContextualOpResult::FallbackRequired`]; it never silently ignores a
/// contextual request by only clearing its lookahead cache.
pub struct TokenStream<'a> {
    inner: TokenStreamInner<'a>,
    buffered_eof_pos: usize,
    peeked: Option<Token>,
    peeked_second: Option<Token>,
    peeked_third: Option<Token>,
    /// Complete lexer checkpoint captured immediately before the head lookahead
    /// token was produced (live backing only). This is the real boundary a
    /// reclassification restores; default checkpoints at a byte position are
    /// never synthesized.
    peek_boundary: Option<LexerCheckpoint>,
    peek_second_boundary: Option<LexerCheckpoint>,
    peek_third_boundary: Option<LexerCheckpoint>,
    /// Boundary of the most recently produced token, moved into a lookahead
    /// slot when a `peek*` method fills it.
    last_boundary: Option<LexerCheckpoint>,
}

impl<'a> TokenStream<'a> {
    /// Create a new token stream from source code.
    pub fn new(input: &'a str) -> Self {
        TokenStream {
            inner: TokenStreamInner::Lexer(Box::new(PerlLexer::new(input))),
            buffered_eof_pos: input.len(),
            peeked: None,
            peeked_second: None,
            peeked_third: None,
            peek_boundary: None,
            peek_second_boundary: None,
            peek_third_boundary: None,
            last_boundary: None,
        }
    }

    /// Create a token stream from a pre-lexed token list.
    ///
    /// This constructor skips lexing entirely and feeds tokens directly from the
    /// provided `Vec`. It is intended for the incremental parsing pipeline where
    /// tokens from a prior parse run can be reused for unchanged regions.
    ///
    /// The stream is *source-less*: it retains no replay authority, so every
    /// classification-level contextual operation
    /// ([`ContextualTokenOp::StatementBoundaryReset`],
    /// [`ContextualTokenOp::ReclassifyFromBoundary`],
    /// [`ContextualTokenOp::EnterFormatBody`]) returns
    /// [`ContextualOpResult::FallbackRequired`] with
    /// [`ContextualFallbackReason::NoBufferedSource`]. A source-less stream is
    /// valid for callers that never request contextual reclassification, or that
    /// handle the typed fallback by rebuilding through a live lexer. Use
    /// [`TokenStream::from_vec_with_source`] when the exact source text is
    /// available.
    ///
    /// # Arguments
    ///
    /// * `tokens` — Pre-lexed tokens. An `Eof` token does **not** need to be
    ///   included; the stream synthesises one when the buffer is exhausted.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::tokens::token_stream::{Token, TokenKind, TokenStream};
    ///
    /// let tokens = vec![
    ///     Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
    ///     Token::new_checked(TokenKind::Eof, "", 2, 2).expect("valid token"),
    /// ];
    /// let mut stream = TokenStream::from_vec(tokens);
    /// assert!(matches!(stream.peek(), Ok(t) if t.kind() == TokenKind::My));
    /// ```
    pub fn from_vec(tokens: Vec<Token>) -> Self {
        Self::buffered(tokens, None)
    }

    /// Create a source-backed token stream from a pre-lexed token list.
    ///
    /// Identical to [`TokenStream::from_vec`] except the stream retains the
    /// exact source text the tokens were produced from. Source identity alone
    /// does not grant replay authority: classification-level contextual
    /// operations still return [`ContextualOpResult::FallbackRequired`], but
    /// with [`ContextualFallbackReason::NoCheckpointAuthority`], because an
    /// exact complete checkpoint at the requested boundary is required for
    /// replay and is only captured by a live pass.
    pub fn from_vec_with_source(tokens: Vec<Token>, source: &'a str) -> Self {
        Self::buffered(tokens, Some(source))
    }

    fn buffered(tokens: Vec<Token>, source: Option<&'a str>) -> Self {
        let buffered_eof_pos = tokens
            .last()
            .map(|token| if token.kind() == TokenKind::Eof { token.start() } else { token.end() })
            .unwrap_or(0);

        TokenStream {
            inner: TokenStreamInner::Buffered(BufferedTokens {
                tokens: VecDeque::from(tokens),
                source,
            }),
            buffered_eof_pos,
            peeked: None,
            peeked_second: None,
            peeked_third: None,
            peek_boundary: None,
            peek_second_boundary: None,
            peek_third_boundary: None,
            last_boundary: None,
        }
    }

    /// Report which backing this stream consumes from.
    #[must_use]
    pub fn backing(&self) -> TokenStreamBacking {
        match &self.inner {
            TokenStreamInner::Lexer(_) => TokenStreamBacking::Live,
            TokenStreamInner::Buffered(buffer) => {
                TokenStreamBacking::Buffered { source_retained: buffer.source.is_some() }
            }
        }
    }

    /// Convert a slice of raw [`LexerToken`]s to parser [`Token`]s, filtering out trivia.
    ///
    /// This is a convenience method for the incremental parsing pipeline where the
    /// token cache stores raw lexer tokens (including whitespace and comments) and
    /// needs to convert them to parser tokens before feeding to [`Self::from_vec`].
    ///
    /// Trivia token types (whitespace, newlines, comments, EOF) are discarded.
    /// All other token types are converted using the same mapping as the live
    /// [`TokenStream`] would apply.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::tokens::token_stream::{TokenKind, TokenStream};
    /// use perl_lexer::{PerlLexer, TokenType};
    ///
    /// // Collect raw lexer tokens
    /// let mut lexer = PerlLexer::new("my $x = 1;");
    /// let mut raw = Vec::new();
    /// while let Some(t) = lexer.next_token() {
    ///     if matches!(t.token_type, TokenType::EOF) { break; }
    ///     raw.push(t);
    /// }
    ///
    /// // Convert to parser tokens and build a stream
    /// let parser_tokens = TokenStream::lexer_tokens_to_parser_tokens(raw);
    /// let mut stream = TokenStream::from_vec(parser_tokens);
    /// assert!(matches!(stream.peek(), Ok(t) if t.kind() == TokenKind::My));
    /// ```
    pub fn lexer_tokens_to_parser_tokens(tokens: Vec<LexerToken>) -> Vec<Token> {
        tokens
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.token_type,
                    LexerTokenType::Whitespace | LexerTokenType::Newline | LexerTokenType::EOF
                ) && !matches!(t.token_type, LexerTokenType::Comment(_))
            })
            .map(Self::convert_lexer_token)
            .collect()
    }

    /// Peek at the next token without consuming it
    pub fn peek(&mut self) -> ParseResult<&Token> {
        if self.peeked.is_none() {
            let token = self.next_token(true)?;
            self.peek_boundary = self.last_boundary.take();
            self.peeked = Some(token);
        }
        self.peeked.as_ref().ok_or(ParseError::UnexpectedEof)
    }

    /// Consume and return the next token
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ParseResult<Token> {
        // If we have a peeked token, return it and shift the peek chain down

        if let Some(token) = self.peeked.take() {
            // Make EOF sticky - if we're returning EOF, put it back in the peek buffer
            // so future peeks still see EOF instead of getting an error
            if token.kind() == TokenKind::Eof {
                self.peeked = Some(token.clone());
            } else {
                self.peeked = self.peeked_second.take();
                self.peek_boundary = self.peek_second_boundary.take();
                self.peeked_second = self.peeked_third.take();
                self.peek_second_boundary = self.peek_third_boundary.take();
            }
            Ok(token)
        } else {
            let token = self.next_token(false)?;
            // The token is consumed immediately; its boundary must not be
            // mistaken for the boundary of a later lookahead fill.
            self.last_boundary = None;
            // Make EOF sticky for fresh tokens too
            if token.kind() == TokenKind::Eof {
                self.peeked = Some(token.clone());
            }
            Ok(token)
        }
    }

    /// Check if we're at the end of input
    pub fn is_eof(&mut self) -> bool {
        matches!(self.peek(), Ok(token) if token.kind() == TokenKind::Eof)
    }

    /// Peek at the second token (two tokens ahead)
    pub fn peek_second(&mut self) -> ParseResult<&Token> {
        // First ensure we have a peeked token
        self.peek()?;

        // If we don't have a second peeked token, get it
        if self.peeked_second.is_none() {
            let token = self.next_token(true)?;
            self.peek_second_boundary = self.last_boundary.take();
            self.peeked_second = Some(token);
        }

        self.peeked_second.as_ref().ok_or(ParseError::UnexpectedEof)
    }

    /// Peek at the third token (three tokens ahead)
    pub fn peek_third(&mut self) -> ParseResult<&Token> {
        // First ensure we have peeked and second peeked tokens
        self.peek_second()?;

        // If we don't have a third peeked token, get it
        if self.peeked_third.is_none() {
            let token = self.next_token(true)?;
            self.peek_third_boundary = self.last_boundary.take();
            self.peeked_third = Some(token);
        }

        self.peeked_third.as_ref().ok_or(ParseError::UnexpectedEof)
    }

    /// Apply a parser-directed contextual token operation and report whether it
    /// actually occurred (issue #8128).
    ///
    /// This is the single typed contract for operations that can change token
    /// classification after the stream has produced a lookahead window:
    ///
    /// - **Live backing**: every classification-level operation applies through
    ///   the real lexer. Reclassification restores the complete checkpoint
    ///   captured immediately before the head lookahead token was produced, so
    ///   all prefix-established state survives; default checkpoints at a byte
    ///   position are never synthesized.
    /// - **Buffered backing**: classification-level operations refuse with
    ///   [`ContextualOpResult::FallbackRequired`] — replay from source requires
    ///   an exact complete checkpoint at the boundary, which the incremental
    ///   layer does not yet retain with the buffer. The refusal leaves the
    ///   stream state untouched so the caller can observe it and rebuild
    ///   through a live lexer. An operation whose requested context verifiably
    ///   already holds on the buffer (the head token is already a format body)
    ///   reports [`ContextualOpResult::NotRequired`] instead. Only
    ///   [`ContextualTokenOp::InvalidateLookahead`] can change buffered stream
    ///   state, because it cannot change classification.
    pub fn apply_contextual(&mut self, operation: ContextualTokenOp) -> ContextualOpResult {
        match operation {
            ContextualTokenOp::InvalidateLookahead => {
                let had_lookahead = self.lookahead_cached();
                self.clear_lookahead();
                match (&self.inner, had_lookahead) {
                    (TokenStreamInner::Lexer(_), true) => ContextualOpResult::AppliedLive,
                    (TokenStreamInner::Buffered(_), true) => ContextualOpResult::AppliedReplay,
                    (_, false) => ContextualOpResult::NotRequired,
                }
            }
            ContextualTokenOp::StatementBoundaryReset => self.apply_statement_boundary_reset(),
            ContextualTokenOp::ReclassifyFromBoundary { expected_context } => {
                self.apply_reclassify_from_boundary(expected_context)
            }
            ContextualTokenOp::EnterFormatBody => self.apply_enter_format_body(),
        }
    }

    /// Apply [`ContextualTokenOp::StatementBoundaryReset`].
    fn apply_statement_boundary_reset(&mut self) -> ContextualOpResult {
        if self.is_buffered() {
            return ContextualOpResult::FallbackRequired {
                reason: self.buffered_fallback_reason(),
            };
        }
        if let Err(reason) = self.restore_live_lookahead_boundary() {
            return ContextualOpResult::FallbackRequired { reason };
        }
        if let TokenStreamInner::Lexer(ref mut lexer) = self.inner {
            // Reset lexer to expect a term (start of new statement). Only the
            // mode is written; every other prefix-established lexer state is
            // preserved.
            lexer.set_mode(LexerMode::ExpectTerm);
        }
        self.clear_lookahead();
        ContextualOpResult::AppliedLive
    }

    /// Apply [`ContextualTokenOp::ReclassifyFromBoundary`].
    fn apply_reclassify_from_boundary(
        &mut self,
        expected_context: LexerMode,
    ) -> ContextualOpResult {
        if !matches!(
            expected_context,
            LexerMode::ExpectTerm | LexerMode::ExpectOperator | LexerMode::ExpectDelimiter
        ) {
            // Body-consumption contexts are positional stream semantics, not
            // token reclassification; `EnterFormatBody` is the format op.
            return ContextualOpResult::Unsupported;
        }
        let has_reclassify_target = matches!(
            self.peeked.as_ref().map(Token::kind),
            Some(kind) if kind != TokenKind::Eof
        );
        if !has_reclassify_target {
            // Nothing in flight to reclassify; the caller can simply peek.
            return ContextualOpResult::NotRequired;
        }
        if self.is_buffered() {
            return ContextualOpResult::FallbackRequired {
                reason: self.buffered_fallback_reason(),
            };
        }

        let boundary = self.peek_boundary.clone();
        if let (TokenStreamInner::Lexer(lexer), Some(boundary)) = (&mut self.inner, boundary) {
            if !lexer.can_restore(&boundary) {
                return ContextualOpResult::FallbackRequired {
                    reason: ContextualFallbackReason::NoCheckpointAuthority,
                };
            }
            // Restore the exact complete state captured before the head token
            // was produced (including heredoc queues, quote operators, and
            // delimiter stacks), then force the requested context.
            lexer.restore(&boundary);
            lexer.set_mode(expected_context);
            self.clear_lookahead();
            return ContextualOpResult::AppliedLive;
        }
        // A live stream always captures boundaries with its lookahead; a miss
        // means there is no real boundary to restore, and synthesizing a
        // default checkpoint at the token position is forbidden (#8128).
        ContextualOpResult::FallbackRequired {
            reason: ContextualFallbackReason::NoCheckpointAuthority,
        }
    }

    /// Apply [`ContextualTokenOp::EnterFormatBody`].
    fn apply_enter_format_body(&mut self) -> ContextualOpResult {
        if self.is_buffered() {
            // The requested context verifiably holds when the buffered head
            // token is already a format body (the producing pass classified
            // it); otherwise the buffered backing cannot re-classify.
            if self.head_kind() == Some(TokenKind::FormatBody) {
                return ContextualOpResult::NotRequired;
            }
            return ContextualOpResult::FallbackRequired {
                reason: self.buffered_fallback_reason(),
            };
        }
        if let Err(reason) = self.restore_live_lookahead_boundary() {
            return ContextualOpResult::FallbackRequired { reason };
        }
        if let TokenStreamInner::Lexer(ref mut lexer) = self.inner {
            lexer.enter_format_mode();
        }
        // Lookahead produced before format entry was classified in the wrong
        // context and must be re-derived from the format body mode.
        self.clear_lookahead();
        ContextualOpResult::AppliedLive
    }

    /// Kind of the head lookahead token, or the first buffered token when no
    /// lookahead window has been produced yet.
    fn head_kind(&self) -> Option<TokenKind> {
        if let Some(token) = self.peeked.as_ref() {
            return Some(token.kind());
        }
        match &self.inner {
            TokenStreamInner::Lexer(_) => None,
            TokenStreamInner::Buffered(buffer) => buffer.tokens.front().map(Token::kind),
        }
    }

    /// Whether the stream is backed by the pre-lexed buffer.
    fn is_buffered(&self) -> bool {
        matches!(self.inner, TokenStreamInner::Buffered(_))
    }

    /// Why the buffered backing cannot honor a classification-level operation.
    fn buffered_fallback_reason(&self) -> ContextualFallbackReason {
        match &self.inner {
            TokenStreamInner::Buffered(buffer) if buffer.source.is_some() => {
                ContextualFallbackReason::NoCheckpointAuthority
            }
            _ => ContextualFallbackReason::NoBufferedSource,
        }
    }

    /// Whether any lookahead slot currently holds a token.
    fn lookahead_cached(&self) -> bool {
        self.peeked.is_some() || self.peeked_second.is_some() || self.peeked_third.is_some()
    }

    /// Restore the complete lexer state captured before the head lookahead.
    ///
    /// Contextual operations clear cached tokens, so a live lexer must rewind
    /// to the captured boundary first or the discarded window would be skipped.
    fn restore_live_lookahead_boundary(&mut self) -> Result<(), ContextualFallbackReason> {
        let Some(boundary) = self.peek_boundary.clone() else {
            return Ok(());
        };
        let TokenStreamInner::Lexer(lexer) = &mut self.inner else {
            return Ok(());
        };
        if !lexer.can_restore(&boundary) {
            return Err(ContextualFallbackReason::NoCheckpointAuthority);
        }
        lexer.restore(&boundary);
        Ok(())
    }

    /// Clear every lookahead slot and its captured boundary atomically.
    fn clear_lookahead(&mut self) {
        self.peeked = None;
        self.peeked_second = None;
        self.peeked_third = None;
        self.peek_boundary = None;
        self.peek_second_boundary = None;
        self.peek_third_boundary = None;
    }

    /// Enter format body parsing mode in the lexer.
    ///
    /// Compatibility wrapper over
    /// [`apply_contextual`](Self::apply_contextual) with
    /// [`ContextualTokenOp::EnterFormatBody`] that discards the typed result.
    /// Production callers should use `apply_contextual` so a buffered stream's
    /// fallback requirement is observable.
    pub fn enter_format_mode(&mut self) {
        let _ = self.apply_contextual(ContextualTokenOp::EnterFormatBody);
    }

    /// Called at statement boundaries to reset lexer state and clear cached lookahead.
    ///
    /// Compatibility wrapper over
    /// [`apply_contextual`](Self::apply_contextual) with
    /// [`ContextualTokenOp::StatementBoundaryReset`] that discards the typed
    /// result. On a buffered stream the operation is refused (the result is
    /// [`ContextualOpResult::FallbackRequired`]) and the stream is untouched.
    pub fn on_stmt_boundary(&mut self) {
        let _ = self.apply_contextual(ContextualTokenOp::StatementBoundaryReset);
    }

    /// Re-lex the current peeked token in `ExpectTerm` mode.
    ///
    /// Compatibility wrapper over
    /// [`apply_contextual`](Self::apply_contextual) with
    /// [`ContextualTokenOp::ReclassifyFromBoundary { expected_context:
    /// LexerMode::ExpectTerm }`] that discards the typed result. The live
    /// backing restores the real captured boundary checkpoint; a buffered
    /// backing refuses without changing state. Production callers should use
    /// `apply_contextual` so the refusal is observable.
    pub fn relex_as_term(&mut self) {
        let _ = self.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm,
        });
    }

    /// Pure peek cache invalidation - no mode changes
    pub fn invalidate_peek(&mut self) {
        self.clear_lookahead();
    }

    /// Convenience method for a one-shot fresh peek
    pub fn peek_fresh_kind(&mut self) -> Option<TokenKind> {
        self.invalidate_peek();
        match self.peek() {
            Ok(token) => Some(token.kind()),
            Err(_) => None,
        }
    }

    /// Get the next token from the backing source.
    fn next_token(&mut self, capture_boundary: bool) -> ParseResult<Token> {
        match &mut self.inner {
            TokenStreamInner::Lexer(lexer) => {
                // Capture the complete lexer state before any trivia is drained
                // so a later reclassification restores the exact boundary the
                // head token was produced from (#8128). Restoring a boundary
                // with a pending heredoc queue is safe here: the checkpoint
                // captures the full queue and the input is unchanged.
                let boundary = capture_boundary.then(|| lexer.checkpoint());
                let token = Self::next_token_from_lexer(lexer)?;
                self.last_boundary = boundary;
                Ok(token)
            }
            TokenStreamInner::Buffered(buffer) => {
                Self::next_token_from_buf(&mut buffer.tokens, &mut self.buffered_eof_pos)
            }
        }
    }

    /// Drain the next non-trivia token from the live lexer.
    fn next_token_from_lexer(lexer: &mut PerlLexer<'_>) -> ParseResult<Token> {
        // Skip whitespace and comments
        loop {
            let lexer_token = lexer.next_token().ok_or(ParseError::UnexpectedEof)?;

            match &lexer_token.token_type {
                LexerTokenType::Whitespace | LexerTokenType::Newline => continue,
                LexerTokenType::Comment(_) => continue,
                LexerTokenType::EOF => {
                    return Ok(token_from_lexer_parts(
                        TokenKind::Eof,
                        "",
                        lexer_token.start,
                        lexer_token.end,
                    ));
                }
                _ => {
                    return Ok(Self::convert_lexer_token(lexer_token));
                }
            }
        }
    }

    /// Return the next token from the pre-lexed buffer.
    fn next_token_from_buf(
        buf: &mut VecDeque<Token>,
        buffered_eof_pos: &mut usize,
    ) -> ParseResult<Token> {
        match buf.pop_front() {
            Some(token) => {
                *buffered_eof_pos =
                    if token.kind() == TokenKind::Eof { token.start() } else { token.end() };
                Ok(token)
            }
            // Synthesise EOF at the most recently known source position.
            None => Ok(Token::eof_at(*buffered_eof_pos)),
        }
    }

    /// Convert a raw lexer token to the parser `Token` type.
    ///
    /// Extracted from `next_token_from_lexer` to keep the match arm readable.
    fn convert_lexer_token(token: LexerToken) -> Token {
        let kind = match &token.token_type {
            // Keywords
            LexerTokenType::Keyword(kw) => match kw.as_ref() {
                "qw" => TokenKind::Identifier, // Keep as identifier but handle specially
                keyword => TokenKind::from_keyword(keyword).unwrap_or(TokenKind::Identifier),
            },

            // Operators
            LexerTokenType::Operator(op) => TokenKind::from_operator(op)
                // Sigils may be surfaced as operator tokens in some contexts.
                .or_else(|| TokenKind::from_sigil(op))
                .unwrap_or(TokenKind::Unknown),

            // Arrow tokens
            LexerTokenType::Arrow => TokenKind::Arrow,
            LexerTokenType::FatComma => TokenKind::FatArrow,

            // Delimiters
            LexerTokenType::LeftParen => TokenKind::LeftParen,
            LexerTokenType::RightParen => TokenKind::RightParen,
            LexerTokenType::LeftBrace => TokenKind::LeftBrace,
            LexerTokenType::RightBrace => TokenKind::RightBrace,
            LexerTokenType::LeftBracket => TokenKind::LeftBracket,
            LexerTokenType::RightBracket => TokenKind::RightBracket,
            LexerTokenType::Semicolon => TokenKind::Semicolon,
            LexerTokenType::Comma => TokenKind::Comma,

            // Division operator (important to handle before other tokens)
            LexerTokenType::Division => TokenKind::Slash,

            // Literals
            LexerTokenType::Number(_) => TokenKind::Number,
            LexerTokenType::StringLiteral | LexerTokenType::InterpolatedString(_) => {
                TokenKind::String
            }
            LexerTokenType::RegexMatch | LexerTokenType::QuoteRegex => TokenKind::Regex,
            LexerTokenType::Substitution => TokenKind::Substitution,
            LexerTokenType::Transliteration => TokenKind::Transliteration,
            LexerTokenType::QuoteSingle => TokenKind::QuoteSingle,
            LexerTokenType::QuoteDouble(_) => TokenKind::QuoteDouble,
            LexerTokenType::QuoteWords => TokenKind::QuoteWords,
            LexerTokenType::QuoteCommand => TokenKind::QuoteCommand,
            LexerTokenType::HeredocStart => TokenKind::HeredocStart,
            LexerTokenType::HeredocBody(_) | LexerTokenType::InterpolatedHeredocBody(_) => {
                TokenKind::HeredocBody
            }
            LexerTokenType::FormatBody(_) => TokenKind::FormatBody,
            LexerTokenType::Version(_) => TokenKind::VString,
            LexerTokenType::DataMarker(_) => TokenKind::DataMarker,
            LexerTokenType::DataBody(_) => TokenKind::DataBody,
            LexerTokenType::UnknownRest => TokenKind::UnknownRest,

            // Identifiers
            LexerTokenType::Identifier(text) => {
                // The lexer emits bare sigil characters ('%', '&') as Identifier
                // tokens in postfix-dereference contexts (e.g. `->%{key}`,
                // `%{$ref}`). Those must map to sigil kinds, NOT operator kinds,
                // so we check sigil priority first for the ambiguous cases.
                // '*' is the exception: as a bare identifier it is multiplication.
                match text.as_ref() {
                    "%" => TokenKind::HashSigil,
                    "&" => TokenKind::SubSigil,
                    _ => TokenKind::from_keyword(text)
                        .or_else(|| TokenKind::from_operator(text))
                        .or_else(|| TokenKind::from_sigil(text))
                        .unwrap_or(TokenKind::Identifier),
                }
            }

            // Handle error tokens that might be valid syntax
            LexerTokenType::Error(msg) => {
                // Check if it's a specific error we want to handle specially
                if msg.as_ref() == "Heredoc nesting too deep" {
                    TokenKind::HeredocDepthLimit
                } else if msg.as_ref().starts_with("unclosed ") {
                    // Unclosed quote-like operator from the lexer (e.g. "unclosed qq delimiter '{'").
                    // Map to the corresponding quote token kind so the parser's quote-handler
                    // produces a proper "Unclosed delimiter" diagnostic rather than the generic
                    // "expected expression, found unknown token" error. q/qq/qw have
                    // unclosed-detection in their primary-expression arms. Substitution
                    // also has strict parser-side validation, as do transliteration
                    // operators, so route malformed `s///`, `tr///`, and `y///`
                    // tokens there instead of losing the lexer diagnostic as Unknown.
                    // Other operators (qr, qx, m) still fall through to Unknown until
                    // dedicated recovery is added.
                    let text = token.text.as_ref();
                    if text.starts_with("qq") {
                        TokenKind::QuoteDouble
                    } else if text.starts_with("qw") {
                        TokenKind::QuoteWords
                    } else if text
                        .strip_prefix('s')
                        .and_then(|rest| rest.chars().next())
                        .is_some_and(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                    {
                        TokenKind::Substitution
                    } else if text
                        .strip_prefix("tr")
                        .and_then(|rest| rest.chars().next())
                        .is_some_and(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                        || text
                            .strip_prefix('y')
                            .and_then(|rest| rest.chars().next())
                            .is_some_and(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                    {
                        TokenKind::Transliteration
                    } else if text
                        .strip_prefix('q')
                        .and_then(|rest| rest.chars().next())
                        .is_some_and(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
                    {
                        TokenKind::QuoteSingle
                    } else {
                        TokenKind::Unknown
                    }
                } else {
                    // Check if it's a brace that the lexer couldn't recognize
                    TokenKind::from_delimiter(token.text.as_ref()).unwrap_or(TokenKind::Unknown)
                }
            }

            _ => TokenKind::Unknown,
        };

        token_from_lexer_parts(kind, token.text, token.start, token.end)
    }
}

/// Convert lexer geometry into a parser token without panicking.
///
/// Ordered spans keep the mapped kind. When lexer `text` is shorter than the
/// span (a trailing newline on `__DATA__` / `__END__`), keep the mapped kind
/// and shrink the span to the text. Reversed or illegally empty geometry
/// becomes a [`TokenKind::Unknown`] token on the ordered span, or EOF at the
/// lower bound if even that constructor is unavailable.
fn token_from_lexer_parts(
    kind: TokenKind,
    text: impl Into<std::sync::Arc<str>>,
    start: usize,
    end: usize,
) -> Token {
    let text = text.into();
    match Token::new_checked(kind, std::sync::Arc::clone(&text), start, end) {
        Ok(token) => token,
        Err(TokenSpanError::TextLengthMismatch { text_len, span_len, .. })
            if start <= end && text_len < span_len =>
        {
            let aligned_end = start.saturating_add(text_len);
            match Token::new_checked(kind, std::sync::Arc::clone(&text), start, aligned_end) {
                Ok(token) => token,
                Err(_) => unknown_or_eof(text, start, end),
            }
        }
        Err(_) => unknown_or_eof(text, start, end),
    }
}

fn unknown_or_eof(text: std::sync::Arc<str>, start: usize, end: usize) -> Token {
    let ordered_start = start.min(end);
    let ordered_end = start.max(end);
    Token::unknown_at(text, ordered_start, ordered_end)
        .unwrap_or_else(|_| Token::eof_at(ordered_start))
}
