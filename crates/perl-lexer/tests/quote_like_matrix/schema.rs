//! Versioned quote-like lexical matrix contract for #7274.

use perl_lexer::{LexerMode, TokenType};

/// Manifest schema identity. Changing accepted row meaning requires a new value.
pub const SCHEMA_VERSION: &str = "quote-like-lexical-matrix/v1";

/// Pinned Perl profile for this matrix. Quote-like forms here are not version-gated.
pub const PERL_PROFILE: &str = "perl5-quote-like-stable";

/// One bounded compile/parse Perl profile row recorded by the oracle harness.
pub const ORACLE_INVOCATION: &str =
    "timeout --signal=KILL 2 env -i PATH=$PATH LC_ALL=C perl -c <tempfile>";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorFamily {
    Q,
    Qq,
    Qw,
    Qx,
    Qr,
    M,
    S,
    Tr,
    Y,
}

impl OperatorFamily {
    pub const ALL: [Self; 9] =
        [Self::Q, Self::Qq, Self::Qw, Self::Qx, Self::Qr, Self::M, Self::S, Self::Tr, Self::Y];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Q => "q",
            Self::Qq => "qq",
            Self::Qw => "qw",
            Self::Qx => "qx",
            Self::Qr => "qr",
            Self::M => "m",
            Self::S => "s",
            Self::Tr => "tr",
            Self::Y => "y",
        }
    }

    pub fn is_two_body(self) -> bool {
        matches!(self, Self::S | Self::Tr | Self::Y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    Attached,
    WhitespaceSeparated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimiterFamily {
    PairedParen,
    PairedBracket,
    PairedBrace,
    PairedAngle,
    UnpairedSlash,
    UnpairedPipe,
    UnpairedHash,
    UnpairedOther,
    MixedPairedToUnpaired,
    MixedDifferentPaired,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceContext {
    Term,
    HashKey,
    Method,
    FatArrow,
    SubroutineName,
    PackageName,
    Label,
    Attribute,
    Prototype,
    Signature,
    HashSlice,
    DivisionOperand,
    DefinedOrOperand,
    FileTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Clean,
    /// Reserved for Error-then-resume rows once #7279 can prove following-code recovery.
    #[expect(dead_code, reason = "issue #7274 disposition vocabulary includes recovered")]
    Recovered,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalStateClass {
    ExpectOperator,
    ExpectTerm,
}

impl TerminalStateClass {
    pub fn matches(self, mode: LexerMode) -> bool {
        match self {
            Self::ExpectOperator => matches!(mode, LexerMode::ExpectOperator),
            Self::ExpectTerm => matches!(mode, LexerMode::ExpectTerm),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedKind {
    QuoteSingle,
    QuoteDouble,
    QuoteWords,
    QuoteCommand,
    QuoteRegex,
    RegexMatch,
    Substitution,
    Transliteration,
    Identifier,
    Keyword,
    Semicolon,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Operator,
    Number,
    StringLiteral,
    Division,
    Error,
    Eof,
}

impl ExpectedKind {
    pub fn matches(self, token_type: &TokenType) -> bool {
        match self {
            Self::QuoteSingle => matches!(token_type, TokenType::QuoteSingle),
            Self::QuoteDouble => matches!(token_type, TokenType::QuoteDouble(_)),
            Self::QuoteWords => matches!(token_type, TokenType::QuoteWords),
            Self::QuoteCommand => matches!(token_type, TokenType::QuoteCommand),
            Self::QuoteRegex => matches!(token_type, TokenType::QuoteRegex),
            Self::RegexMatch => matches!(token_type, TokenType::RegexMatch),
            Self::Substitution => matches!(token_type, TokenType::Substitution),
            Self::Transliteration => matches!(token_type, TokenType::Transliteration),
            Self::Identifier => matches!(token_type, TokenType::Identifier(_)),
            Self::Keyword => matches!(token_type, TokenType::Keyword(_)),
            Self::Semicolon => matches!(token_type, TokenType::Semicolon),
            Self::LeftParen => matches!(token_type, TokenType::LeftParen),
            Self::RightParen => matches!(token_type, TokenType::RightParen),
            Self::LeftBrace => matches!(token_type, TokenType::LeftBrace),
            Self::RightBrace => matches!(token_type, TokenType::RightBrace),
            Self::Operator => {
                matches!(
                    token_type,
                    TokenType::Operator(_) | TokenType::Arrow | TokenType::FatComma
                )
            }
            Self::Number => matches!(token_type, TokenType::Number(_)),
            Self::StringLiteral => matches!(token_type, TokenType::StringLiteral),
            Self::Division => matches!(token_type, TokenType::Division),
            Self::Error => matches!(token_type, TokenType::Error(_)),
            Self::Eof => matches!(token_type, TokenType::EOF),
        }
    }

    pub fn is_quote_like(self) -> bool {
        matches!(
            self,
            Self::QuoteSingle
                | Self::QuoteDouble
                | Self::QuoteWords
                | Self::QuoteCommand
                | Self::QuoteRegex
                | Self::RegexMatch
                | Self::Substitution
                | Self::Transliteration
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpec {
    pub kind: ExpectedKind,
    pub text: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextOrdinary {
    Present { kind: ExpectedKind, text: &'static str },
    EatenByError,
    EatenByComment,
    NoneAtEof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleExpectation {
    CompileAccept,
    CompileReject,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    AttachedPaired,
    AttachedUnpaired,
    ImmediateHash,
    WhitespaceSeparated,
    CommentGapBeforePaired,
    ConsecutiveCommentGap,
    CommentBetweenBodies,
    MixedSecondDelimiter,
    NestedPaired,
    EscapedDelimiter,
    EmptyBody,
    MultilineLf,
    MultilineCrlf,
    MultilineCr,
    Unicode,
    Modifier,
    MalformedFollower,
    HashKey,
    Method,
    FatArrow,
    SubroutineName,
    PackageName,
    Label,
    HashSlice,
    Division,
    DefinedOr,
    FileTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixRow {
    pub id: &'static str,
    pub schema_version: &'static str,
    pub perl_profile: &'static str,
    pub operator: OperatorFamily,
    pub spelling: Spelling,
    pub delimiter: DelimiterFamily,
    pub source_context: SourceContext,
    pub source: &'static str,
    pub expected: &'static [TokenSpec],
    pub next_ordinary: NextOrdinary,
    pub terminal: TerminalStateClass,
    pub disposition: Disposition,
    pub oracle: OracleExpectation,
    pub limitation: Option<&'static str>,
    pub axes: &'static [Axis],
}

pub const fn spec(kind: ExpectedKind, text: &'static str) -> TokenSpec {
    TokenSpec { kind, text }
}
