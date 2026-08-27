use std::ops::Range;

use crate::TokenKind;

/// Byte span carried by a [`crate::Token`].
///
/// Geometry fields are private. External crates cannot construct a reversed
/// span through safe public Rust: use [`TokenSpan::try_new`].
///
/// This struct is `#[non_exhaustive]`: additional fields may be added later
/// without a breaking change. Downstream crates must not rely on struct
/// literals or exhaustive field patterns. That marker is the #2898 evolution
/// disposition for [`TokenSpan`].
///
/// ```compile_fail
/// use perl_token::TokenSpan;
/// let mut span = TokenSpan::try_new(0, 1).unwrap();
/// span.start = 2;
/// ```
///
/// ```compile_fail
/// use perl_token::TokenSpan;
/// let mut span = TokenSpan::try_new(0, 1).unwrap();
/// span.end = 0;
/// ```
///
/// ```compile_fail
/// use perl_token::TokenSpan;
/// let _ = TokenSpan::new(2, 1);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    start: usize,
    end: usize,
}

impl TokenSpan {
    /// Create a span, returning an error when `end < start`.
    ///
    /// Empty spans (`start == end`) are valid at this layer. Token-level empty
    /// span policy is enforced by [`crate::Token::new_checked`].
    pub const fn try_new(start: usize, end: usize) -> Result<Self, TokenSpanError> {
        if end < start {
            return Err(TokenSpanError::EndBeforeStart { start, end });
        }

        Ok(Self::from_ordered(start, end))
    }

    /// Crate-private constructor for spans already proven ordered.
    ///
    /// Callers must guarantee `end >= start`. This is the residual E02
    /// unchecked path after public constructors were sealed; it is not part of
    /// the public API and must not be re-exported. Workspace-wide constructor
    /// migration remains #8660.
    ///
    /// ```compile_fail
    /// use perl_token::TokenSpan;
    /// let _ = TokenSpan::from_ordered(0, 1);
    /// ```
    pub(crate) const fn from_ordered(start: usize, end: usize) -> Self {
        debug_assert!(end >= start);
        Self { start, end }
    }

    /// Create a span from a [`Range`].
    ///
    /// # Errors
    ///
    /// Returns [`TokenSpanError::EndBeforeStart`] when `range.end < range.start`.
    pub const fn try_from_range(range: Range<usize>) -> Result<Self, TokenSpanError> {
        Self::try_new(range.start, range.end)
    }

    /// Starting byte offset (inclusive).
    pub const fn start(self) -> usize {
        self.start
    }

    /// Ending byte offset (exclusive).
    pub const fn end(self) -> usize {
        self.end
    }

    /// Span length in bytes.
    ///
    /// Reversed spans cannot be constructed through the public API, so this is
    /// ordinary subtraction rather than saturating recovery.
    pub const fn len(self) -> usize {
        debug_assert!(self.end >= self.start);
        self.end - self.start
    }

    /// Whether the span length is zero bytes.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Convert this span to a standard `Range`.
    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }

    /// Return whether `offset` is inside this half-open span.
    ///
    /// The start is inclusive and the end is exclusive, matching Rust
    /// [`Range`] semantics. Empty spans contain no offsets.
    pub const fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Return whether `offset` touches this span, including the end boundary.
    ///
    /// This is useful for cursor-oriented callers that need positions at token
    /// boundaries to resolve to the adjacent token. Empty spans touch exactly
    /// their single boundary offset.
    pub const fn touches(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }

    /// Return whether this span overlaps `other`.
    ///
    /// Spans are treated as half-open byte ranges, so adjacent spans such as
    /// `0..2` and `2..4` do not overlap. Empty spans never overlap.
    pub const fn overlaps(self, other: Self) -> bool {
        !self.is_empty() && !other.is_empty() && self.start < other.end && other.start < self.end
    }

    /// Return the smallest span covering both spans.
    pub const fn cover(self, other: Self) -> Self {
        Self::from_ordered(min_usize(self.start, other.start), max_usize(self.end, other.end))
    }
}

const fn min_usize(left: usize, right: usize) -> usize {
    if left <= right { left } else { right }
}

const fn max_usize(left: usize, right: usize) -> usize {
    if left >= right { left } else { right }
}

/// Error type for checked token/span constructors.
///
/// This enum is `#[non_exhaustive]`: new invariant failures may be added as
/// constructors gain source-geometry checks. Downstream matches must include a
/// wildcard arm. That marker is the #2898 evolution disposition for
/// [`TokenSpanError`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSpanError {
    /// End offset is before start offset.
    EndBeforeStart {
        /// Start byte offset that was supplied.
        start: usize,
        /// End byte offset that violated `end >= start`.
        end: usize,
    },
    /// Empty span is only valid for EOF or explicit synthetic tokens.
    EmptySpanNotAllowed {
        /// Token kind that disallows an empty span.
        kind: TokenKind,
        /// Byte offset where the empty span was constructed.
        at: usize,
    },
    /// Token text byte length does not equal the span width.
    TextLengthMismatch {
        /// `text` length in bytes.
        text_len: usize,
        /// `end - start` span width in bytes.
        span_len: usize,
        /// Start byte offset that was supplied.
        start: usize,
        /// End byte offset that was supplied.
        end: usize,
    },
}

impl std::fmt::Display for TokenSpanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EndBeforeStart { start, end } => {
                write!(f, "token span invariant violated: end ({end}) < start ({start})")
            }
            Self::EmptySpanNotAllowed { kind, at } => {
                write!(f, "empty span not allowed for token kind {kind:?} at byte {at}")
            }
            Self::TextLengthMismatch { text_len, span_len, start, end } => {
                write!(
                    f,
                    "token text length ({text_len}) != span width ({span_len}) at {start}..{end}"
                )
            }
        }
    }
}

impl std::error::Error for TokenSpanError {}

/// Kinds that may occupy a zero-length span.
#[inline]
pub(crate) const fn allows_empty_span(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Eof | TokenKind::Unknown)
}

#[inline]
pub(crate) const fn validate_non_empty_span(
    kind: TokenKind,
    start: usize,
    is_empty: bool,
) -> Result<(), TokenSpanError> {
    if is_empty && !allows_empty_span(kind) {
        return Err(TokenSpanError::EmptySpanNotAllowed { kind, at: start });
    }

    Ok(())
}

#[inline]
pub(crate) fn validate_text_span_width(
    text_len: usize,
    span: TokenSpan,
) -> Result<(), TokenSpanError> {
    if text_len != span.len() {
        return Err(TokenSpanError::TextLengthMismatch {
            text_len,
            span_len: span.len(),
            start: span.start(),
            end: span.end(),
        });
    }

    Ok(())
}
