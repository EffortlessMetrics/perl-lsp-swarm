use std::{ops::Range, sync::Arc};

use crate::span::{allows_empty_span, validate_non_empty_span, validate_text_span_width};
use crate::{TokenKind, TokenSpan, TokenSpanError};

/// Borrowed view over token data for allocation-sensitive paths.
///
/// Unlike [`Token`], this type borrows source text and does not allocate.
/// Convert to [`Token`] explicitly with [`TokenRef::to_owned_token`] or `From`.
///
/// Geometry fields are private. `text` remains a public field because replacing
/// it cannot create a reversed or illegally empty span. `kind` is a read
/// accessor: assignment on an empty EOF would otherwise bypass empty-span
/// policy. Use [`TokenRef::new_checked`] / [`TokenRef::with_kind`] to change
/// kind.
///
/// This struct is `#[non_exhaustive]`: additional fields may be added later.
/// Downstream crates cannot use struct literals. That marker is the #2898
/// evolution disposition for [`TokenRef`].
///
/// ```compile_fail
/// use perl_token::{TokenKind, TokenRef};
/// let mut tok = TokenRef::new_checked(TokenKind::Identifier, "x", 0, 1).unwrap();
/// tok.start = 4;
/// ```
///
/// ```compile_fail
/// use perl_token::{TokenKind, TokenRef};
/// let mut tok = TokenRef::new_checked(TokenKind::Identifier, "x", 0, 1).unwrap();
/// tok.end = 0;
/// ```
///
/// ```compile_fail
/// use perl_token::{TokenKind, TokenRef};
/// let _ = TokenRef::new(TokenKind::Identifier, "x", 1, 0);
/// ```
///
/// ```compile_fail
/// use perl_token::{TokenKind, TokenRef};
/// let mut tok = TokenRef::new_checked(TokenKind::Eof, "", 0, 0).unwrap();
/// tok.kind = TokenKind::Semicolon;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenRef<'src> {
    kind: TokenKind,
    /// Borrowed source text slice.
    pub text: &'src str,
    start: usize,
    end: usize,
}

impl<'src> TokenRef<'src> {
    /// Create a borrowed token view while enforcing span invariants.
    ///
    /// Rules:
    /// - `start <= end`
    /// - zero-length spans are accepted for EOF and explicit synthetic unknown tokens
    /// - `text.len()` must equal `end - start`, except for the explicit
    ///   geometry-only `UnknownRest` representation (empty text over a
    ///   non-empty span), whose payload-free shape *is* the recovery signal
    ///
    /// [`TokenRef::try_new`] is an alias of this constructor.
    pub fn new_checked(
        kind: TokenKind,
        text: &'src str,
        start: usize,
        end: usize,
    ) -> Result<Self, TokenSpanError> {
        let span = TokenSpan::try_new(start, end)?;
        validate_non_empty_span(kind, span.start(), span.is_empty())?;
        if !(kind == TokenKind::UnknownRest && text.is_empty() && !span.is_empty()) {
            validate_text_span_width(text.len(), span)?;
        }
        Ok(Self { kind, text, start: span.start(), end: span.end() })
    }

    /// Create a borrowed token view with checked span invariants.
    ///
    /// This is the same constructor as [`TokenRef::new_checked`].
    pub fn try_new(
        kind: TokenKind,
        text: &'src str,
        start: usize,
        end: usize,
    ) -> Result<Self, TokenSpanError> {
        Self::new_checked(kind, text, start, end)
    }

    /// Token classification for parser matching.
    pub const fn kind(self) -> TokenKind {
        self.kind
    }

    /// Starting byte position.
    pub const fn start(self) -> usize {
        self.start
    }

    /// Ending byte position.
    pub const fn end(self) -> usize {
        self.end
    }

    /// Return the token span length in bytes.
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Return whether the token span is empty.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Return whether this is a payload-free `UnknownRest` recovery view:
    /// empty text over a non-empty span, the honest "the remainder is
    /// unparsed" geometry of the budget-stop recovery contract (#6717).
    pub fn is_geometry_only(self) -> bool {
        self.kind == TokenKind::UnknownRest && self.text.is_empty() && self.start < self.end
    }

    /// Return the token span.
    pub const fn span(self) -> TokenSpan {
        TokenSpan::from_ordered(self.start, self.end)
    }

    /// Return a human-readable display name for this token.
    pub fn display_name(self) -> &'static str {
        self.kind.display_name()
    }

    /// Convert this borrowed token view into an owned [`Token`].
    ///
    /// A payload-free `UnknownRest` view round-trips as the geometry-only
    /// recovery representation instead of being rejected: collapsing it to
    /// `Eof` would erase the typed `lexer_budget_exhausted` stop cause
    /// downstream (#14158). A payload-carrying `UnknownRest` view (the bounded
    /// unterminated-heredoc recovery shape) round-trips its text losslessly
    /// instead of being silently emptied.
    pub fn to_owned_token(self) -> Token {
        if self.kind == TokenKind::UnknownRest && self.text.is_empty() {
            return match Token::unknown_rest_at(self.start, self.end) {
                Ok(token) => token,
                Err(_) => Token::eof_at(self.start),
            };
        }
        Token::from_valid_parts(self.kind, Arc::from(self.text), self.start, self.end)
    }

    /// Clone this view with a new token kind, enforcing empty-span policy.
    pub fn with_kind(self, kind: TokenKind) -> Result<Self, TokenSpanError> {
        Self::new_checked(kind, self.text, self.start, self.end)
    }
}

/// Token produced by the lexer and consumed by the parser.
///
/// Stores the token kind, original source text, and byte span. The text is kept
/// in an `Arc<str>` so buffering and lookahead can clone tokens cheaply.
///
/// Geometry fields are private. Safe external code cannot create reversed or
/// illegally empty tokens: use [`Token::new_checked`], [`Token::eof_at`], or
/// [`Token::unknown_at`]. `text` remains a public field for Arc sharing;
/// replacing it cannot violate span geometry. `kind` is a read accessor so
/// assignment on an empty EOF cannot bypass empty-span policy; use
/// [`Token::with_kind`] to change kind.
///
/// This struct is `#[non_exhaustive]`: additional fields may be added later.
/// Downstream crates cannot use struct literals. That marker is the #2898
/// evolution disposition for [`Token`].
///
/// # Examples
///
/// ```rust
/// use perl_token::{Token, TokenKind};
///
/// let tok = Token::new_checked(TokenKind::Sub, "sub", 0, 3)?;
/// assert_eq!(tok.kind(), TokenKind::Sub);
/// assert_eq!(&*tok.text, "sub");
/// assert_eq!(tok.start(), 0);
/// assert_eq!(tok.end(), 3);
/// # Ok::<(), perl_token::TokenSpanError>(())
/// ```
///
/// ```compile_fail
/// use perl_token::{Token, TokenKind};
/// let _ = Token {
///     kind: TokenKind::Identifier,
///     text: "x".into(),
///     start: 1,
///     end: 0,
/// };
/// ```
///
/// ```compile_fail
/// use perl_token::{Token, TokenKind};
/// let _ = Token::new(TokenKind::Identifier, "x", 1, 0);
/// ```
///
/// ```compile_fail
/// use perl_token::{Token, TokenKind};
/// let mut tok = Token::new_checked(TokenKind::Identifier, "x", 0, 1).unwrap();
/// tok.start = 4;
/// ```
///
/// ```compile_fail
/// use perl_token::{Token, TokenKind};
/// let mut tok = Token::new_checked(TokenKind::Identifier, "x", 0, 1).unwrap();
/// tok.end = 0;
/// ```
///
/// ```compile_fail
/// use perl_token::{Token, TokenKind};
/// let mut tok = Token::eof_at(0);
/// tok.kind = TokenKind::Semicolon;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    kind: TokenKind,
    /// Original source text for precise reconstruction.
    pub text: Arc<str>,
    start: usize,
    end: usize,
}

impl Token {
    /// Create a token while enforcing span invariants.
    ///
    /// Rules:
    /// - `start <= end`
    /// - zero-length spans are accepted for EOF and explicit synthetic unknown tokens
    /// - `text.len()` must equal `end - start`, except for the explicit
    ///   geometry-only `UnknownRest` representation (empty text over a
    ///   non-empty span), whose payload-free shape *is* the recovery signal
    ///
    /// [`Token::try_new`] is an alias of this constructor.
    pub fn new_checked(
        kind: TokenKind,
        text: impl Into<Arc<str>>,
        start: usize,
        end: usize,
    ) -> Result<Self, TokenSpanError> {
        let span = TokenSpan::try_new(start, end)?;
        validate_non_empty_span(kind, span.start(), span.is_empty())?;
        let text = text.into();
        if !(kind == TokenKind::UnknownRest && text.is_empty() && !span.is_empty()) {
            validate_text_span_width(text.as_ref().len(), span)?;
        }
        Ok(Self::from_valid_parts(kind, text, span.start(), span.end()))
    }

    /// Create a token with checked span invariants.
    ///
    /// This is the same constructor as [`Token::new_checked`].
    pub fn try_new(
        kind: TokenKind,
        text: impl Into<Arc<str>>,
        start: usize,
        end: usize,
    ) -> Result<Self, TokenSpanError> {
        Self::new_checked(kind, text, start, end)
    }

    /// Create a token from a previously validated [`TokenSpan`].
    pub fn try_from_span(
        kind: TokenKind,
        text: impl Into<Arc<str>>,
        span: TokenSpan,
    ) -> Result<Self, TokenSpanError> {
        Self::new_checked(kind, text, span.start(), span.end())
    }

    /// Module-private constructor used only after span invariants are proven.
    ///
    /// This is the residual E02 unchecked path after public constructors were
    /// sealed. It is not part of the public or crate-external API. Workspace-wide
    /// constructor migration remains #8660.
    ///
    /// ```compile_fail
    /// use perl_token::{Token, TokenKind};
    /// use std::sync::Arc;
    /// let _ = Token::from_valid_parts(TokenKind::Identifier, Arc::from("x"), 0, 1);
    /// ```
    pub(crate) fn from_valid_parts(
        kind: TokenKind,
        text: Arc<str>,
        start: usize,
        end: usize,
    ) -> Self {
        debug_assert!(end >= start);
        debug_assert!(end > start || allows_empty_span(kind));
        debug_assert!(
            text.as_ref().len() == end.saturating_sub(start)
                || (kind == TokenKind::UnknownRest && text.as_ref().is_empty() && start < end)
        );
        Self { kind, text, start, end }
    }

    /// Create an EOF token at `pos`.
    pub fn eof_at(pos: usize) -> Self {
        Self::from_valid_parts(TokenKind::Eof, Arc::from(""), pos, pos)
    }

    /// Create an unknown (synthetic) token at `start..end`.
    ///
    /// Empty unknown tokens are allowed and must still have empty text. Reversed
    /// spans are rejected rather than silently clamped.
    pub fn unknown_at(
        text: impl Into<Arc<str>>,
        start: usize,
        end: usize,
    ) -> Result<Self, TokenSpanError> {
        Self::new_checked(TokenKind::Unknown, text, start, end)
    }

    /// Create a payload-free `UnknownRest` token over `start..end`.
    ///
    /// The lexer uses this representation when a budget prevents it from
    /// retaining the remaining source. The span is exact; the public `text`
    /// field is deliberately empty, so the payload-free geometry carries the
    /// "remainder is unparsed" signal that the parser's typed
    /// `lexer_budget_exhausted` stop cause is keyed on (#14158). Empty and
    /// reversed spans are rejected.
    pub fn unknown_rest_at(start: usize, end: usize) -> Result<Self, TokenSpanError> {
        Self::new_checked(TokenKind::UnknownRest, Arc::from(""), start, end)
    }

    /// Token classification for parser matching.
    pub const fn kind(&self) -> TokenKind {
        self.kind
    }

    /// Starting byte position.
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Ending byte position.
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Return this token's byte span.
    pub const fn span(&self) -> TokenSpan {
        TokenSpan::from_ordered(self.start, self.end)
    }

    /// Return this token's byte span as `Range<usize>`.
    pub const fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    /// Project the public token components without exposing geometry mutation.
    pub fn as_parts(&self) -> (TokenKind, &str, usize, usize) {
        (self.kind, self.text.as_ref(), self.start, self.end)
    }

    /// Clone this token with a new checked span.
    pub fn with_span(&self, start: usize, end: usize) -> Result<Self, TokenSpanError> {
        Self::new_checked(self.kind, Arc::clone(&self.text), start, end)
    }

    /// Clone this token with a new token kind, enforcing empty-span policy.
    pub fn with_kind(&self, kind: TokenKind) -> Result<Self, TokenSpanError> {
        Self::new_checked(kind, Arc::clone(&self.text), self.start, self.end)
    }

    /// Return the token span length in bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_token::{Token, TokenKind};
    ///
    /// let tok = Token::new_checked(TokenKind::Identifier, "foo", 10, 13)?;
    /// assert_eq!(tok.len(), 3);
    /// # Ok::<(), perl_token::TokenSpanError>(())
    /// ```
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Return whether the token span is empty.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_token::{Token, TokenKind};
    ///
    /// let tok = Token::eof_at(8);
    /// assert!(tok.is_empty());
    /// ```
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Return whether this is a payload-free `UnknownRest` recovery token:
    /// empty text over a non-empty span. The payload-free geometry is the
    /// recording signal for the parser's typed `lexer_budget_exhausted`
    /// stop cause (#14158); it must survive lexer-to-parser conversion
    /// instead of being collapsed to a silent `Eof`.
    pub fn is_geometry_only(&self) -> bool {
        self.kind == TokenKind::UnknownRest && self.text.is_empty() && self.start < self.end
    }

    /// Return a human-readable display name for this token.
    pub fn display_name(&self) -> &'static str {
        self.kind.display_name()
    }

    /// Return a borrowed token view over this token.
    pub fn as_ref_token(&self) -> TokenRef<'_> {
        TokenRef { kind: self.kind, text: self.text.as_ref(), start: self.start, end: self.end }
    }
}

impl From<TokenRef<'_>> for Token {
    fn from(value: TokenRef<'_>) -> Self {
        value.to_owned_token()
    }
}
