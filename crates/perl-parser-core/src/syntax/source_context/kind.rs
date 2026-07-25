//! Classification kinds for non-code source regions.

/// Lexical classification of a source byte span.
///
/// `Code` is the default when no enclosing non-code region covers an offset.
/// Unclosed literals at EOF are classified as [`Self::RecoveryAmbiguous`] so
/// consumers can fail closed instead of treating partial input as executable code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceRegionKind {
    /// Executable Perl source (default; usually not stored in the index).
    Code,
    /// Line comment from `#` through end-of-line outside literals.
    LineComment,
    /// POD documentation block (`=pod` … `=cut` at column 0).
    Pod,
    /// `__DATA__` / `__END__` tail including marker and payload.
    DataSection,
    /// Single-, double-, or backtick-quoted string literal.
    StringLiteral,
    /// `q{}`, `qq//`, `qw//`, `qx//`, and related quote-like bodies.
    QuoteLike,
    /// `m//`, `qr//`, `s///`, `tr///`, bare `/…/`, and related regex bodies.
    RegexLike,
    /// Heredoc body between opener and closing delimiter line.
    Heredoc,
    /// Unclosed or ambiguous literal/recovery input.
    RecoveryAmbiguous,
}

impl SourceRegionKind {
    /// Stable snake_case name for tracing and serde-style logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::LineComment => "line_comment",
            Self::Pod => "pod",
            Self::DataSection => "data_section",
            Self::StringLiteral => "string_literal",
            Self::QuoteLike => "quote_like",
            Self::RegexLike => "regex_like",
            Self::Heredoc => "heredoc",
            Self::RecoveryAmbiguous => "recovery_ambiguous",
        }
    }
}
