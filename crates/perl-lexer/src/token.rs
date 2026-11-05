//! Token types and structures for the Perl lexer.
//!
//! [`TokenType`] classifies every token the lexer can emit, and [`Token`]
//! bundles a type with its source text and byte span.

use std::sync::Arc;

/// Classification of one [`StringPart`] interpolation segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringPartKind {
    /// Literal text run (raw source bytes, including escapes as written).
    Literal,
    /// Variable interpolation: `$var`, `@array`, `%hash`.
    Variable,
    /// Expression interpolation: `${expr}`, `@{expr}`.
    Expression,
    /// Method/subscript tail: `->method()`, `->[0]`, etc.
    MethodCall,
    /// Array/hash slice tail: `[1..3]`, `{key}` after a variable head.
    ArraySlice,
}

/// Raw lexical spelling plus absolute source byte range for one segment (#6730).
///
/// `text` is always the exact `source[start..end]` bytes captured during the
/// original lexer scan. No cooked/decoded alternate is stored today; escapes
/// remain backslash-prefixed in literal runs.
#[derive(Debug, Clone, Eq)]
pub struct SegmentPayload {
    /// Raw source spelling for this segment.
    pub text: Arc<str>,
    /// Absolute start byte (inclusive) in the lexer's input.
    pub start: usize,
    /// Absolute end byte (exclusive) in the lexer's input.
    pub end: usize,
}

impl PartialEq for SegmentPayload {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.start == other.start && self.end == other.end
    }
}

/// Parts of an interpolated string with authoritative source geometry (#6730).
#[derive(Debug, Clone, Eq)]
pub enum StringPart {
    /// Literal text
    Literal(SegmentPayload),
    /// Variable interpolation: $var, @array, %hash
    Variable(SegmentPayload),
    /// Expression interpolation: ${expr}, @{expr}
    Expression(SegmentPayload),
    /// Method call: `->method()`
    MethodCall(SegmentPayload),
    /// Array slice: [1..3]
    ArraySlice(SegmentPayload),
}

impl PartialEq for StringPart {
    /// Compare segment kind and raw text only (geometry is asserted separately).
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && self.text() == other.text()
    }
}

impl StringPart {
    /// Build a segment from an authoritative source slice.
    pub fn from_source(kind: StringPartKind, source: &str, start: usize, end: usize) -> Self {
        let payload = SegmentPayload { text: Arc::from(&source[start..end]), start, end };
        match kind {
            StringPartKind::Literal => Self::Literal(payload),
            StringPartKind::Variable => Self::Variable(payload),
            StringPartKind::Expression => Self::Expression(payload),
            StringPartKind::MethodCall => Self::MethodCall(payload),
            StringPartKind::ArraySlice => Self::ArraySlice(payload),
        }
    }

    /// Segment kind.
    pub fn kind(&self) -> StringPartKind {
        match self {
            Self::Literal(_) => StringPartKind::Literal,
            Self::Variable(_) => StringPartKind::Variable,
            Self::Expression(_) => StringPartKind::Expression,
            Self::MethodCall(_) => StringPartKind::MethodCall,
            Self::ArraySlice(_) => StringPartKind::ArraySlice,
        }
    }

    /// Raw lexical spelling (identical to `source[start..end]` at emission).
    pub fn text(&self) -> &str {
        match self {
            Self::Literal(p)
            | Self::Variable(p)
            | Self::Expression(p)
            | Self::MethodCall(p)
            | Self::ArraySlice(p) => p.text.as_ref(),
        }
    }

    /// Absolute start byte (inclusive) in the source file.
    pub fn start(&self) -> usize {
        match self {
            Self::Literal(p)
            | Self::Variable(p)
            | Self::Expression(p)
            | Self::MethodCall(p)
            | Self::ArraySlice(p) => p.start,
        }
    }

    /// Absolute end byte (exclusive) in the source file.
    pub fn end(&self) -> usize {
        match self {
            Self::Literal(p)
            | Self::Variable(p)
            | Self::Expression(p)
            | Self::MethodCall(p)
            | Self::ArraySlice(p) => p.end,
        }
    }

    /// Absolute source byte range for this segment.
    pub fn source_range(&self) -> std::ops::Range<usize> {
        self.start()..self.end()
    }

    /// Expectation helper when tests assert kind/text without caring about offsets.
    pub fn expect(kind: StringPartKind, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        let len = text.len();
        Self::from_source(kind, text.as_ref(), 0, len)
    }
}

/// Token types for Perl
#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Slash-derived tokens
    /// Division operator: /
    Division,
    /// Regex match: m// or //
    RegexMatch,
    /// Substitution: s///
    Substitution,
    /// Transliteration: tr/// or y///
    Transliteration,
    /// Quote regex: qr//
    QuoteRegex,

    // String and quote tokens
    /// String literal: "string" or 'string'
    StringLiteral,
    /// Single quote: q// (non-interpolating; invariant under the
    /// interpolation setting)
    QuoteSingle,
    /// Double quote: qq// with the body's string parts (#8779).
    ///
    /// Per the #8244/#8779 interpolation policy: when `parse_interpolation`
    /// is enabled the body is segmented during the original body scan; when
    /// disabled the body is one opaque `StringPart::Literal`. The token
    /// identity is unchanged — qq stays qq.
    QuoteDouble(Vec<StringPart>),
    /// Quote words: qw//
    QuoteWords,
    /// Quote command: qx// or `backticks`
    ///
    /// Intentional interpolation boundary (#8779): command bodies stay
    /// opaque under every interpolation configuration.
    QuoteCommand,

    // String interpolation tokens
    /// String with interpolated parts
    InterpolatedString(Vec<StringPart>),

    // Heredoc tokens
    /// Heredoc start: <<EOF or <<'EOF'
    HeredocStart,
    /// Heredoc body content (non-interpolating bodies, and interpolating
    /// bodies outside the interpolation setting's authority): geometry only.
    HeredocBody(Arc<str>),
    /// Interpolating heredoc body with string parts (#8779).
    ///
    /// Emitted for bareword and `<<"EOF"` bodies: segmented when
    /// `parse_interpolation` is enabled, one opaque `StringPart::Literal`
    /// of the whole body when disabled. `<<'EOF'`, `<<\EOF`, and backtick
    /// bodies never consume the setting and stay `HeredocBody` (control
    /// invariance).
    InterpolatedHeredocBody(Vec<StringPart>),

    // Format declarations
    /// Format body content
    FormatBody(Arc<str>),

    // Version strings
    /// Version string: v5.32.0
    Version(Arc<str>),

    // POD documentation
    /// POD documentation block
    Pod,

    // Data sections
    /// Data section marker: __DATA__ or __END__
    DataMarker(Arc<str>),
    /// Data section body content
    DataBody(Arc<str>),

    // Error recovery
    /// Unknown rest of input (used when budget exceeded)
    UnknownRest,

    // Identifiers and literals
    /// Identifier or variable name
    Identifier(Arc<str>),
    /// Numeric literal
    Number(Arc<str>),
    /// Operator
    Operator(Arc<str>),
    /// Keyword
    Keyword(Arc<str>),

    // Delimiters
    /// Left parenthesis: (
    LeftParen,
    /// Right parenthesis: )
    RightParen,
    /// Left bracket: [
    LeftBracket,
    /// Right bracket: ]
    RightBracket,
    /// Left brace: {
    LeftBrace,
    /// Right brace: }
    RightBrace,

    // Punctuation
    /// Semicolon: ;
    Semicolon,
    /// Comma: ,
    Comma,
    /// Colon: :
    Colon,
    /// Arrow: ->
    Arrow,
    /// Fat comma: =>
    FatComma,

    // Whitespace and comments
    /// Whitespace (usually not returned)
    Whitespace,
    /// Newline character
    Newline,
    /// Comment text
    Comment(Arc<str>),

    // Special tokens
    /// End of file
    EOF,
    /// Error token for invalid input
    Error(Arc<str>),
}

impl TokenType {
    /// Return `true` when this token is ignorable trivia.
    pub fn is_trivia(&self) -> bool {
        matches!(self, Self::Whitespace | Self::Newline | Self::Comment(_))
    }

    /// Return `true` when lexing could not continue safely.
    pub fn is_recovery_token(&self) -> bool {
        matches!(self, Self::UnknownRest | Self::Error(_))
    }
}

/// A single token produced by [`PerlLexer`](crate::PerlLexer).
///
/// Carries its [`TokenType`], the original source text (as a cheap-to-clone
/// `Arc<str>`), and the byte span within the input.
#[derive(Debug, Clone)]
pub struct Token {
    /// Classification of this token (keyword, operator, literal, etc.).
    pub token_type: TokenType,
    /// Original source text that this token spans.
    pub text: Arc<str>,
    /// Starting byte offset (inclusive) in the source input.
    pub start: usize,
    /// Ending byte offset (exclusive) in the source input.
    pub end: usize,
}

impl Token {
    /// Create a new token with the given type, source text, and byte span.
    pub fn new(token_type: TokenType, text: impl Into<Arc<str>>, start: usize, end: usize) -> Self {
        Self { token_type, text: text.into(), start, end }
    }

    /// Return the byte length of this token's span (`end - start`).
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Return `true` if the token has a zero-length span.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, TokenType};
    use std::sync::Arc;

    #[test]
    fn token_type_trivia_classifier_matches_expected_variants()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(TokenType::Whitespace.is_trivia());
        assert!(TokenType::Newline.is_trivia());
        assert!(TokenType::Comment(Arc::from("# note")).is_trivia());
        assert!(!TokenType::Identifier(Arc::from("foo")).is_trivia());
        Ok(())
    }

    #[test]
    fn token_type_recovery_classifier_matches_expected_variants()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(TokenType::UnknownRest.is_recovery_token());
        assert!(TokenType::Error(Arc::from("oops")).is_recovery_token());
        assert!(!TokenType::EOF.is_recovery_token());
        Ok(())
    }

    // --- Token::new ---

    #[test]
    fn token_new_stores_type_text_and_span() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::Semicolon, ";", 3, 4);
        assert_eq!(tok.token_type, TokenType::Semicolon);
        assert_eq!(tok.text.as_ref(), ";");
        assert_eq!(tok.start, 3);
        assert_eq!(tok.end, 4);
        Ok(())
    }

    #[test]
    fn token_new_accepts_arc_str_directly() -> Result<(), Box<dyn std::error::Error>> {
        let text: Arc<str> = Arc::from("hello");
        let tok = Token::new(TokenType::Identifier(Arc::from("hello")), text, 0, 5);
        assert_eq!(tok.start, 0);
        assert_eq!(tok.end, 5);
        Ok(())
    }

    // --- Token::len ---

    #[test]
    fn token_len_returns_end_minus_start() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::Semicolon, ";", 10, 17);
        assert_eq!(tok.len(), 7);
        Ok(())
    }

    #[test]
    fn token_len_zero_when_span_is_empty() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::EOF, "", 5, 5);
        assert_eq!(tok.len(), 0);
        Ok(())
    }

    #[test]
    fn token_len_single_byte_span() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::Comma, ",", 0, 1);
        assert_eq!(tok.len(), 1);
        Ok(())
    }

    // --- Token::is_empty ---

    #[test]
    fn token_is_empty_true_for_zero_length_span() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::EOF, "", 0, 0);
        assert!(tok.is_empty());
        Ok(())
    }

    #[test]
    fn token_is_empty_true_for_non_zero_start_matching_end()
    -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::EOF, "", 42, 42);
        assert!(tok.is_empty());
        Ok(())
    }

    #[test]
    fn token_is_empty_false_for_non_zero_length_span() -> Result<(), Box<dyn std::error::Error>> {
        let tok = Token::new(TokenType::Semicolon, ";", 0, 1);
        assert!(!tok.is_empty());
        Ok(())
    }
}
